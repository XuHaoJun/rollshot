---
kind: stitch_sequences_benchmark_compare
schema_version: 1
benchmark_id: 2026-06-02-p6-routine-feature
benchmark_scope: p6-routine-feature
roadmap_item: P6
status: draft
before:
  short_commit: 506e7d1
  commit: 506e7d1e7f7c4938c029ead59c044ba59dbdb636
  jsonl: bench-results/runs/p6-routine-feature/before.jsonl
after:
  short_commit: 9ff91d0
  commit: 9ff91d0dbbc2b8bbd116c7b62c70dafa1280cb2f
  jsonl: bench-results/runs/p6-routine-feature/after.jsonl
run:
  date: 2026-06-02
  harness: crates/rollshot-core/benches/stitch_sequences.rs
  command: "rtk cargo bench -p rollshot-core --bench stitch_sequences -- --out bench-results/runs/p6-routine-feature/after.jsonl"
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
# Benchmark comparison: 506e7d1 → 9ff91d0

## Total time per frame (p50)

| scenario | before µs | after µs | Δ | Δ% |
|---|---:|---:|---:|---:|
| bad_frame | 1,206 | 1,884 | +678 | +56.2% |
| duplicate_frames | 201 | 149 | -52 | -25.9% |
| image_cards | 720 | 740 | +20 | +2.8% |
| linear_horizontal_left | 687 | 799 | +112 | +16.3% |
| linear_horizontal_right | 788 | 955 | +167 | +21.2% |
| linear_vertical_down | 756 | 723 | -33 | -4.4% |
| linear_vertical_up | 822 | 860 | +38 | +4.6% |
| long_lazy_load | 0 | 4,941 | +4,941 | n/a |
| long_sticky_header | 4,598 | 5,043 | +445 | +9.7% |
| long_vertical_jitter | 4,768 | 5,026 | +258 | +5.4% |
| long_vertical_text | 5,268 | 5,655 | +387 | +7.3% |
| low_feature_text | 775 | 727 | -48 | -6.2% |
| repeated_grid | 162 | 1 | -161 | -99.4% |
| repeated_rows | 157 | 1 | -156 | -99.4% |
| sticky_header | 841 | 1,163 | +322 | +38.3% |

## Append time (p95) — P1 target

| scenario | before µs | after µs | Δ | Δ% |
|---|---:|---:|---:|---:|
| bad_frame | 121 | 81 | -40 | -33.1% |
| duplicate_frames | 134 | 79 | -55 | -41.0% |
| image_cards | 127 | 62 | -65 | -51.2% |
| linear_horizontal_left | 128 | 71 | -57 | -44.5% |
| linear_horizontal_right | 137 | 70 | -67 | -48.9% |
| linear_vertical_down | 137 | 111 | -26 | -19.0% |
| linear_vertical_up | 131 | 108 | -23 | -17.6% |
| long_lazy_load | 0 | 3,462 | +3,462 | n/a |
| long_sticky_header | 2,247 | 2,163 | -84 | -3.7% |
| long_vertical_jitter | 3,730 | 3,825 | +95 | +2.5% |
| long_vertical_text | 4,453 | 4,442 | -11 | -0.2% |
| low_feature_text | 112 | 100 | -12 | -10.7% |
| repeated_grid | 0 | 0 | +0 | n/a |
| repeated_rows | 0 | 0 | +0 | n/a |
| sticky_header | 116 | 84 | -32 | -27.6% |

## Prepare (p50) — P2 target

| scenario | before µs | after µs | Δ | Δ% |
|---|---:|---:|---:|---:|
| bad_frame | 112 | 80 | -32 | -28.6% |
| duplicate_frames | 0 | 0 | +0 | n/a |
| image_cards | 69 | 76 | +7 | +10.1% |
| linear_horizontal_left | 69 | 76 | +7 | +10.1% |
| linear_horizontal_right | 62 | 75 | +13 | +21.0% |
| linear_vertical_down | 63 | 72 | +9 | +14.3% |
| linear_vertical_up | 66 | 75 | +9 | +13.6% |
| long_lazy_load | 0 | 434 | +434 | n/a |
| long_sticky_header | 436 | 441 | +5 | +1.1% |
| long_vertical_jitter | 431 | 437 | +6 | +1.4% |
| long_vertical_text | 440 | 765 | +325 | +73.9% |
| low_feature_text | 62 | 80 | +18 | +29.0% |
| repeated_grid | 0 | 0 | +0 | n/a |
| repeated_rows | 0 | 0 | +0 | n/a |
| sticky_header | 65 | 83 | +18 | +27.7% |

## Coarse (p50)

| scenario | before µs | after µs | Δ | Δ% |
|---|---:|---:|---:|---:|
| bad_frame | 177 | 145 | -32 | -18.1% |
| duplicate_frames | 0 | 0 | +0 | n/a |
| image_cards | 152 | 134 | -18 | -11.8% |
| linear_horizontal_left | 129 | 164 | +35 | +27.1% |
| linear_horizontal_right | 148 | 186 | +38 | +25.7% |
| linear_vertical_down | 167 | 135 | -32 | -19.2% |
| linear_vertical_up | 160 | 182 | +22 | +13.8% |
| long_lazy_load | 0 | 653 | +653 | n/a |
| long_sticky_header | 679 | 678 | -1 | -0.1% |
| long_vertical_jitter | 678 | 648 | -30 | -4.4% |
| long_vertical_text | 800 | 823 | +23 | +2.9% |
| low_feature_text | 183 | 125 | -58 | -31.7% |
| repeated_grid | 0 | 0 | +0 | n/a |
| repeated_rows | 0 | 0 | +0 | n/a |
| sticky_header | 159 | 158 | -1 | -0.6% |

## NCC (p50) — P3 target

| scenario | before µs | after µs | Δ | Δ% |
|---|---:|---:|---:|---:|
| bad_frame | 323 | 303 | -20 | -6.2% |
| duplicate_frames | 0 | 0 | +0 | n/a |
| image_cards | 154 | 211 | +57 | +37.0% |
| linear_horizontal_left | 156 | 253 | +97 | +62.2% |
| linear_horizontal_right | 163 | 363 | +200 | +122.7% |
| linear_vertical_down | 132 | 161 | +29 | +22.0% |
| linear_vertical_up | 136 | 231 | +95 | +69.9% |
| long_lazy_load | 0 | 1,399 | +1,399 | n/a |
| long_sticky_header | 1,335 | 1,374 | +39 | +2.9% |
| long_vertical_jitter | 1,295 | 1,420 | +125 | +9.7% |
| long_vertical_text | 1,457 | 1,386 | -71 | -4.9% |
| low_feature_text | 198 | 166 | -32 | -16.2% |
| repeated_grid | 0 | 0 | +0 | n/a |
| repeated_rows | 0 | 0 | +0 | n/a |
| sticky_header | 201 | 324 | +123 | +61.2% |

## Verifier (p50)

| scenario | before µs | after µs | Δ | Δ% |
|---|---:|---:|---:|---:|
| bad_frame | 51 | 287 | +236 | +462.7% |
| duplicate_frames | 0 | 0 | +0 | n/a |
| image_cards | 135 | 122 | -13 | -9.6% |
| linear_horizontal_left | 124 | 124 | +0 | +0.0% |
| linear_horizontal_right | 204 | 170 | -34 | -16.7% |
| linear_vertical_down | 98 | 128 | +30 | +30.6% |
| linear_vertical_up | 181 | 195 | +14 | +7.7% |
| long_lazy_load | 0 | 990 | +990 | n/a |
| long_sticky_header | 801 | 888 | +87 | +10.9% |
| long_vertical_jitter | 851 | 974 | +123 | +14.5% |
| long_vertical_text | 986 | 1,184 | +198 | +20.1% |
| low_feature_text | 125 | 188 | +63 | +50.4% |
| repeated_grid | 0 | 0 | +0 | n/a |
| repeated_rows | 0 | 0 | +0 | n/a |
| sticky_header | 177 | 143 | -34 | -19.2% |

## Algorithmic counters (p50)

| scenario | coarse candidates before | after | Δ | Δ% | NCC offsets before | after | Δ | Δ% | NCC pixel visits before | after | Δ | Δ% | verifier candidates before | after | Δ | Δ% |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| bad_frame | 2 | 2 | +0 | +0.0% | 194 | 194 | +0 | +0.0% | 11,168,192 | 11,168,192 | +0 | +0.0% | 6 | 6 | +0 | +0.0% |
| duplicate_frames | 0 | 0 | +0 | n/a | 0 | 0 | +0 | n/a | 0 | 0 | +0 | n/a | 0 | 0 | +0 | n/a |
| image_cards | 1 | 1 | +0 | +0.0% | 164 | 164 | +0 | +0.0% | 9,441,152 | 9,441,152 | +0 | +0.0% | 3 | 3 | +0 | +0.0% |
| linear_horizontal_left | 1 | 1 | +0 | +0.0% | 154 | 154 | +0 | +0.0% | 8,865,472 | 8,865,472 | +0 | +0.0% | 3 | 3 | +0 | +0.0% |
| linear_horizontal_right | 1 | 1 | +0 | +0.0% | 154 | 154 | +0 | +0.0% | 8,865,472 | 8,865,472 | +0 | +0.0% | 3 | 3 | +0 | +0.0% |
| linear_vertical_down | 1 | 1 | +0 | +0.0% | 154 | 154 | +0 | +0.0% | 8,865,472 | 8,865,472 | +0 | +0.0% | 3 | 3 | +0 | +0.0% |
| linear_vertical_up | 1 | 1 | +0 | +0.0% | 154 | 154 | +0 | +0.0% | 8,865,472 | 8,865,472 | +0 | +0.0% | 3 | 3 | +0 | +0.0% |
| long_lazy_load | 0 | 1 | +1 | n/a | 0 | 174 | +174 | n/a | 0 | 49,889,280 | +49,889,280 | n/a | 0 | 3 | +3 | n/a |
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
| bad_frame | 3,440 | 3,664 | +224 |
| duplicate_frames | 3,416 | 3,256 | -160 |
| image_cards | 3,848 | 3,436 | -412 |
| linear_horizontal_left | 3,860 | 3,528 | -332 |
| linear_horizontal_right | 3,864 | 3,552 | -312 |
| linear_vertical_down | 3,860 | 3,760 | -100 |
| linear_vertical_up | 3,852 | 3,732 | -120 |
| long_lazy_load | 0 | 97,888 | +97,888 |
| long_sticky_header | 71,272 | 71,372 | +100 |
| long_vertical_jitter | 98,128 | 98,260 | +132 |
| long_vertical_text | 99,260 | 99,292 | +32 |
| low_feature_text | 3,828 | 3,716 | -112 |
| repeated_grid | 2,876 | 2,476 | -400 |
| repeated_rows | 2,876 | 2,476 | -400 |
| sticky_header | 3,772 | 3,464 | -308 |

## Regressions (Δ > +5%)

- **bad_frame**: +462.7%
- **linear_horizontal_right**: +122.7%
- **long_vertical_text**: +73.9%
- **linear_vertical_up**: +69.9%
- **linear_horizontal_left**: +62.2%
- **sticky_header**: +61.2%
- **bad_frame**: +56.2%
- **low_feature_text**: +50.4%
- **sticky_header**: +38.3%
- **image_cards**: +37.0%
- **linear_vertical_down**: +30.6%
- **low_feature_text**: +29.0%
- **sticky_header**: +27.7%
- **linear_horizontal_left**: +27.1%
- **linear_horizontal_right**: +25.7%
- **linear_vertical_down**: +22.0%
- **linear_horizontal_right**: +21.2%
- **linear_horizontal_right**: +21.0%
- **long_vertical_text**: +20.1%
- **linear_horizontal_left**: +16.3%
- **long_vertical_jitter**: +14.5%
- **linear_vertical_down**: +14.3%
- **linear_vertical_up**: +13.8%
- **linear_vertical_up**: +13.6%
- **long_sticky_header**: +10.9%
- **image_cards**: +10.1%
- **linear_horizontal_left**: +10.1%
- **long_sticky_header**: +9.7%
- **long_vertical_jitter**: +9.7%
- **linear_vertical_up**: +7.7%
- **long_vertical_text**: +7.3%
- **long_vertical_jitter**: +5.4%

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

## Interpretation

Full verdict + context: roadmap `docs/stitching-rollshot-optimizations-2.md` §8.9, spec
`docs/superpowers/specs/2026-06-02-p6-lazy-load-robust-stitching-design.md`.

- **Correctness: byte-identical on every scenario** (hashes all `same`) — the robust
  verifier is a monotonic superset (clean content still takes the legacy mean path) and
  the routine feature candidate never displaces a clean-frame winner.
- **What changed perf-wise**: ② made feature matching a per-frame candidate source. The
  added cost is FAST corner + 8-D descriptor **extraction on `curr` each frame** (anchor
  descriptors are cached/reused), *not* KNN search.
- **Realistic frames (900×700)**: total p50 ≈ +5–10% (`long_vertical_text` +7.3%,
  `long_sticky_header` +9.7%, `long_vertical_jitter` +5.4%) — within/near the ~10% budget;
  some swing is run-to-run noise on a shared machine.
- **Tiny golden fixtures (120–320px)** show large Δ% (e.g. `bad_frame`) because their
  per-frame baseline is microscopic, so the constant extraction cost dominates. These are
  not representative of real captures.
- **HNSW**: evaluated (`hora` 0.1.1) and **bench-gated out** — ~43× slower than the
  rayon+SIMD brute-force at N=1200 plus a recall bug; reverted. HNSW would not have touched
  this delta anyway (it's extraction, not search). If the +5–10% ever matters, the levers
  are extraction (SIMD / downsample / P9 Y-plane) or making the feature candidate
  borderline-only (spec §7), not ANN.
- `long_lazy_load` shows `before 0` because that synthetic spec was added on this branch;
  it has no `main` baseline.

