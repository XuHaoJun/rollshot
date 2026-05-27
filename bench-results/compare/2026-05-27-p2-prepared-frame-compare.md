---
kind: stitch_sequences_benchmark_compare
schema_version: 1
benchmark_id: 2026-05-27-p2-prepared-frame
benchmark_scope: p2-prepared-frame
roadmap_item: P2
status: user_accepted
before:
  short_commit: 63c6a9e
  commit: 63c6a9e8a1cf454716fc463b77012fd3fef3a675
  jsonl: bench-results/runs/p2-prepared-frame/before.jsonl
after:
  short_commit: 86ed5b6
  commit: 86ed5b6cf013d86d4c3cc79693d7e6a106bdf4ec
  jsonl: bench-results/runs/p2-prepared-frame/after.jsonl
run:
  date: 2026-05-27
  harness: crates/rollshot-core/benches/stitch_sequences.rs
  command: "rtk cargo bench -p rollshot-core --bench stitch_sequences -- --fixtures long_vertical_text,long_sticky_header,long_vertical_jitter --repeats 3 --out bench-results/runs/p2-prepared-frame/after.jsonl"
  fixtures:
    - long_vertical_text
    - long_sticky_header
    - long_vertical_jitter
  repeats: 3
environment:
  os: Linux-6.8.0-117-generic-x86_64-with-glibc2.39
  architecture: x86_64
  cpu_model: "Intel(R) Core(TM) Ultra 7 265K"
  logical_cpus: 8
notes:
  - "Raw JSONL files are local benchmark artifacts under bench-results/runs/ and are not intended to be committed."
  - "Peak RSS is allocator- and machine-dependent; compare trends on this machine."
  - "After bench ran on a doc-only HEAD identical in code to 86ed5b6; the after SHA records that code state."
---
# Benchmark comparison: 63c6a9e → 86ed5b6

## Total time per frame (p50)

| scenario | before µs | after µs | Δ | Δ% |
|---|---:|---:|---:|---:|
| long_sticky_header | 14,192 | 12,323 | -1,869 | -13.2% |
| long_vertical_jitter | 13,197 | 11,672 | -1,525 | -11.6% |
| long_vertical_text | 13,751 | 12,065 | -1,686 | -12.3% |

## Append time (p95) — P1 target

| scenario | before µs | after µs | Δ | Δ% |
|---|---:|---:|---:|---:|
| long_sticky_header | 2,165 | 2,386 | +221 | +10.2% |
| long_vertical_jitter | 3,681 | 3,987 | +306 | +8.3% |
| long_vertical_text | 4,247 | 4,554 | +307 | +7.2% |

## Prepare (p50) — P2 target

| scenario | before µs | after µs | Δ | Δ% |
|---|---:|---:|---:|---:|
| long_sticky_header | 977 | 442 | -535 | -54.8% |
| long_vertical_jitter | 938 | 437 | -501 | -53.4% |
| long_vertical_text | 950 | 446 | -504 | -53.1% |

## Coarse (p50)

| scenario | before µs | after µs | Δ | Δ% |
|---|---:|---:|---:|---:|
| long_sticky_header | 1,302 | 858 | -444 | -34.1% |
| long_vertical_jitter | 1,274 | 804 | -470 | -36.9% |
| long_vertical_text | 1,296 | 919 | -377 | -29.1% |

## NCC (p50) — P3 target

| scenario | before µs | after µs | Δ | Δ% |
|---|---:|---:|---:|---:|
| long_sticky_header | 9,073 | 8,688 | -385 | -4.2% |
| long_vertical_jitter | 8,060 | 8,057 | -3 | -0.0% |
| long_vertical_text | 8,238 | 8,100 | -138 | -1.7% |

## Verifier (p50)

| scenario | before µs | after µs | Δ | Δ% |
|---|---:|---:|---:|---:|
| long_sticky_header | 830 | 873 | +43 | +5.2% |
| long_vertical_jitter | 837 | 891 | +54 | +6.5% |
| long_vertical_text | 988 | 1,017 | +29 | +2.9% |

## Peak RSS Δ (kB)

| scenario | before kB | after kB | Δ kB |
|---|---:|---:|---:|
| long_sticky_header | 70,024 | 71,276 | +1,252 |
| long_vertical_jitter | 96,944 | 98,128 | +1,184 |
| long_vertical_text | 98,004 | 99,260 | +1,256 |

## Regressions (Δ > +5%)

- **long_sticky_header**: +10.2%
- **long_vertical_jitter**: +8.3%
- **long_vertical_text**: +7.2%
- **long_vertical_jitter**: +6.5%
- **long_sticky_header**: +5.2%

## Output correctness drift

| scenario | before hash | after hash | diff? |
|---|---|---|---|
| long_sticky_header | `fd57675e84120080` | `fd57675e84120080` | same |
| long_vertical_jitter | `420a65270e9feaaf` | `420a65270e9feaaf` | same |
| long_vertical_text | `5c0f3290b99f1b97` | `5c0f3290b99f1b97` | same |

## Interpretation

P2 goal met. Output is byte-identical on all three fixtures, `prepare_frame_us`
(p50) drops **-53~-55%** (above the 30–50% target), `coarse_us` (p50) drops
**-29~-37%** as a bonus (prev coarse is now cached too), and net total per-frame
(p50) improves **-12~-13%**. NCC is flat (P2 does not touch it), as expected.

The `append_us` p95 (+7~10%), `verifier_us` p50 (+3~6%), and peak RSS (+~1.2 MB)
regressions are a **real, reproducible trade-off, not noise** — they appear with
the same sign across all three fixtures. P2 keeps the `last_good` anchor's
derived buffers (gray f32 ≈ 2.5 MB, coarse, projections) resident across frames
instead of recomputing them; the RSS bump confirms this. Append (large RGBA
memcpy) and the verifier (pixel scan) are memory-bandwidth bound, so the larger
resident working set costs a few % in cache pressure. This is the intended
"trade resident memory for recomputed CPU" shape: the prepare/coarse savings far
outweigh the append/verifier cost, so end-to-end p50 still improves.

This report supersedes an earlier run whose `before` baseline was captured under
heavier machine load (prepare ≈ 1,450 µs vs ≈ 950 µs here), which overstated the
improvement (prepare -69%, total -19%) and understated the append regression. The
`after` state matched across both runs (prepare ≈ 440 µs); these back-to-back
same-session numbers are the honest figures.

