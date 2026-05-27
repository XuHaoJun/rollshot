# P3 — Fast NCC with Fused Single-Pass `wide` SIMD (Design)

> Roadmap source: `docs/stitching-rollshot-optimizations-2.md` §5 (P3). This
> spec is **live** for the duration of the P3 workflow. It is the source of
> truth until the branch lands, after which it becomes a frozen snapshot.

## Goal

Reduce the cost of the full-resolution template NCC refine stage without
changing matcher semantics.

Today `ncc_score_shifted` scans the clipped overlap twice for every refinement
offset: once to compute means and once to compute covariance/variance. P3
changes this to fast normalized cross-correlation:

- a single fused `wide::f32x8` pass over the clipped overlap accumulates all
  five sums at once (`sum_x`, `sum_x2`, `sum_y`, `sum_y2`, `sum_xy`);
- final NCC is computed from those sums.

No integral image is built: because `sum_xy` must scan every pixel of the rect
anyway, the four normalization sums are accumulated in that same pass for free.
This avoids ~168MB of resident f64 tables on a retina pair (prev + curr) plus a
full-frame build pass, with no per-offset asymptotic loss.

This is a performance change only. Candidate ranking may differ by small
floating-point tolerance, but verifier-facing outcomes and golden outputs must
not regress.

## Decisions

- Raise the workspace MSRV only if `wide 1.4.0` actually requires it, and then
  only to the minimum version it needs. Verify with `cargo check` on the current
  floor (1.85); do not bump to 1.89 speculatively.
- Add `wide = "1.4.0"` as a normal `rollshot-core` dependency.
- Do not add a SIMD feature flag. The production path uses `wide` by default;
  `wide` itself falls back to a software implementation where there is no
  hardware SIMD, so no separate scalar path is maintained.
- Do not require `RUSTFLAGS="-C target-cpu=native"` or non-baseline target
  features for release builds.
- Keep the old two-pass NCC implementation only under `#[cfg(test)]` for
  equivalence tests.
- Do not build an integral image. Compute all five NCC sums in one fused `wide`
  pass over the overlap rect; `PreparedFrame` gains no new field.
- Reject windows whose variance is `≤ 1.0` (matching legacy `ncc_score_shifted`),
  not `≤ 1e-9`, so low-texture matcher outcomes are preserved.

## Current State (Verified Against Code, 2026-05-27)

- P0 benchmark harness exists: `crates/rollshot-core/benches/stitch_sequences.rs`
  emits `template_ncc_us`, `ncc_offsets_scored`, `ncc_pixel_visits`, output
  correctness fields, and summary rows. `scripts/bench/compare.py` emits
  markdown compare reports under `bench-results/compare/`.
- P1 `StripCanvas` and P2 `PreparedFrame` cache have landed. Recent history
  includes `perf: P2 prepared frame cache (#17)`.
- The template refine stage lives in `crates/rollshot-core/src/matcher.rs`.
  `search_template_axis` scores each refinement offset in rayon, then sorts by
  score and offset.
- `ncc_score_shifted` is the P3 hot function. It computes overlap/region
  clipping, scans for means, then scans again for numerator and variances.
- `PreparedFrame` currently owns RGBA, duplicate signature, eager `gray`, lazy
  coarse samples, and lazy edge projections. P3 adds no new cached field — the
  fused NCC pass reads the existing `gray` buffer directly.
- `rollshot-core` currently depends on `image`, `imageproc`, and `rayon`. It has
  no `[features]` section.

## Platform and Dependency Notes

`wide` 1.4.0 is acceptable for P3 (raise MSRV only if it requires it; see
Decisions). Its documented platform model is:

- explicit SIMD on `x86`, `x86_64`, `wasm32`, and `aarch64 neon`;
- LLVM/autovec or scalar fallback elsewhere;
- build-time feature selection only, not runtime CPU feature detection.

For rollshot this means:

- Linux x86_64, macOS Intel, macOS Apple Silicon, and future Windows x86_64 are
  valid targets for the `wide` backend.
- Distributed binaries must use baseline target features. Local benchmarks may
  optionally test `target-cpu=native`, but that is not part of the acceptance
  gate.
- P3 correctness tests must not assume a particular hardware SIMD instruction
  set. `wide` may compile to scalar on some targets and must still produce
  valid NCC scores.

## Architecture

### `NccSums`

The fused pass returns a small struct carrying the five sums — no summed-area
table:

```rust
struct NccSums {
    n: f64,
    sum_x: f64,
    sum_x2: f64,
    sum_y: f64,
    sum_y2: f64,
    sum_xy: f64,
}
```

Sums accumulate in `f64` (lane reduction plus the scalar tail) to avoid
accumulation error on larger frames. The NCC-specific helpers can live in
`matcher.rs` for the first P3 patch; if the file becomes harder to reason about,
move them into a private `ncc.rs` module. Do not widen public API solely for
this.

### `PreparedFrame`

`PreparedFrame` is **not** extended. There is no integral cache and no new
field; the fused pass reads the existing `gray()` buffer for both frames. An
integral cache was considered and rejected (see Goal): it would add ~168MB
resident on a retina pair for no per-offset speedup, since `sum_xy` scans the
rect regardless.

### Matcher Flow

Change the template refine path to pass `&PreparedFrame` instead of parallel
gray slices:

```rust
fn template_candidates(
    prev: &PreparedFrame,
    curr: &PreparedFrame,
    locked_axis: Option<ScrollAxis>,
    last_motion: (i32, i32),
    coarse: &[MotionCandidate],
    config: &StitchConfig,
    metrics: &mut StitchMetrics,
) -> Vec<MotionCandidate>
```

`search_template_axis` can then access:

- `prev.gray()` and `curr.gray()` for the fused sums pass;
- dimensions from `PreparedFrame`.

This keeps derived data ownership coherent and avoids threading four related
buffers through every helper.

## Fast NCC Algorithm

`fast_ncc_score_shifted` preserves the existing public behavior of
`ncc_score_shifted`:

1. Call `compute_overlap(width, height, width, height, dx, dy)`.
2. Clip the previous-frame overlap to `region`.
3. Return `f32::MIN` if the clipped rectangle is empty.
4. Convert the previous-frame rectangle to the matching current-frame
   rectangle by subtracting `(dx, dy)`.
5. Run one fused `wide` pass over the rect, producing `NccSums` with
   `n = rect_w * rect_h` and `sum_x`, `sum_x2`, `sum_y`, `sum_y2`, `sum_xy`.
6. Compute NCC from the sums (`ncc_from_sums`):

```rust
let numerator = sum_xy - (sum_x * sum_y / n);
let prev_var = sum_x2 - (sum_x * sum_x / n);
let curr_var = sum_y2 - (sum_y * sum_y / n);

// var here is the sum-of-squared-deviations (Σx² − (Σx)²/n) — the same
// quantity and scale legacy `ncc_score_shifted` thresholds on. Use legacy's
// ≤ 1.0 floor (NOT 1e-9) so near-flat / low-texture windows that legacy
// rejected are not silently scored — preserving matcher outcomes.
if prev_var <= 1.0 || curr_var <= 1.0 {
    return f32::MIN;
}

(numerator / (prev_var * curr_var).sqrt()) as f32
```

The production function returns `f32::MIN` on unusable windows, matching current
`ncc_score_shifted` behavior. `ncc_from_sums` is split out from the fused pass so
the formula and its threshold are unit-testable without building frames.

## Fused `wide` Sums

Implement the five sums as one row-contiguous pass:

```rust
fn fused_sums_wide(
    prev_gray: &[f32],
    curr_gray: &[f32],
    width: usize,
    prev_rect: Region,
    curr_rect: Region,
) -> NccSums
```

For each row, slice `prev`/`curr` to the rect width and iterate with
`chunks_exact(8)`:

- per 8-pixel chunk, build two `f32x8` and accumulate `sum_x`, `sum_x2`,
  `sum_y`, `sum_y2`, `sum_xy` into five `f32x8` lane accumulators;
- finish the `< 8` `chunks_exact(...).remainder()` tail in scalar `f64`;
- reduce the SIMD lanes to `f64` and add the scalar tail.

Use `chunks_exact(8)` rather than indexed reads: the slice makes the in-bounds
length provable, so the compiler elides the per-lane bounds checks — important
because the workspace sets `unsafe_code = "forbid"`, so `get_unchecked` is
unavailable. Build each vector with
`wide::f32x8::from(<[f32; 8]>::try_from(chunk).unwrap())`.

The helper is intentionally private. P3 does not create a general SIMD
abstraction layer.

## Metrics

Keep existing metric fields:

- `template_ncc_us`
- `ncc_offsets_scored`
- `ncc_pixel_visits`

`ncc_offsets_scored` remains exact. `ncc_pixel_visits` should continue to
represent logical pixels compared, not physical loop visits. It may stay as the
current structural estimate (`offsets.len() * region_area`) so benchmark
comparisons remain compatible with earlier P0/P2 reports.

There is no integral build to attribute; the fused pass is the only NCC work for
a frame pair and is already inside `template_ncc_us`. The fused pass visits each
pixel once (legacy visited twice), so the `#[cfg(test)]` budget counter
`full_res_ncc_pixel_visits` drops and stays under its existing ceiling.

## Benchmark Gate

P3 is not complete without before/after benchmark artifacts following
`docs/bench.md`.

Required local raw artifacts:

```text
bench-results/runs/p3-fast-ncc/before.jsonl
bench-results/runs/p3-fast-ncc/after.jsonl
```

Required committed compare artifact:

```text
bench-results/compare/2026-05-27-p3-fast-ncc-compare.md
```

The compare report must be generated with frontmatter, for example:

```bash
rtk python3 scripts/bench/compare.py \
    --include-frontmatter \
    --benchmark-id 2026-05-27-p3-fast-ncc \
    --benchmark-scope p3-fast-ncc \
    --roadmap-item P3 \
    --status user_accepted \
    --date 2026-05-27 \
    --command "rtk cargo bench -p rollshot-core --bench stitch_sequences -- --out bench-results/runs/p3-fast-ncc/after.jsonl" \
    bench-results/runs/p3-fast-ncc/before.jsonl \
    bench-results/runs/p3-fast-ncc/after.jsonl \
    > bench-results/compare/2026-05-27-p3-fast-ncc-compare.md
```

Raw JSONL files remain local by default because `bench-results/runs/` is
gitignored. Commit the compare markdown unless the reviewer explicitly asks to
version the raw JSONL.

The report must specifically address `template_ncc_us` p50, which is the NCC
field emitted by `scripts/bench/compare.py`. A successful P3 should reduce NCC
time on NCC-heavy scenarios. If the measured improvement is small, the report
must explain whether rayon overhead, memory bandwidth, SIMD lane utilization, or
scenario mix likely dominated.

## Tests

Add focused unit tests near matcher tests:

- `ncc_from_sums_rejects_low_variance` (the `≤ 1.0` floor, tested directly)
- `ncc_from_sums_matches_pearson_for_known_vectors`
- `fast_ncc_matches_legacy_ncc_across_widths_and_axes`
- `fast_ncc_rejects_low_variance_windows_like_legacy`
- `fast_ncc_preserves_best_offset_on_synthetic_scroll`
- `fast_ncc_handles_constant_windows_as_no_score`

`fast_ncc_matches_legacy_ncc_across_widths_and_axes` must (a) use rect widths
that are NOT multiples of 8 so the scalar tail is exercised, and (b) cover both
vertical (`dx=0`) and horizontal (`dy=0`) shifts. Compare with a tolerance, not
exact equality: a starting `1e-4` is reasonable because legacy NCC accumulates in
`f32` while fused NCC uses `f64` sums and a different SIMD reduction order for
`sum_xy`. Repeated-grid ambiguity and full-pipeline outcomes are covered by the
existing golden-fixture tests, so no new dedicated repeated-grid unit test is
required.

Existing tests that must keep passing:

- prepared-frame cache tests from P2;
- matcher axis/direction tests;
- repeated-grid ambiguity test;
- golden fixture tests;
- metrics population tests.

Verification commands:

```bash
rtk cargo test
rtk cargo fmt --check
rtk cargo clippy --workspace --all-targets -- -D warnings
rtk cargo bench -p rollshot-core --bench stitch_sequences -- \
    --out bench-results/runs/p3-fast-ncc/after.jsonl
rtk python3 scripts/bench/compare.py \
    bench-results/runs/p3-fast-ncc/before.jsonl \
    bench-results/runs/p3-fast-ncc/after.jsonl
```

## Acceptance Criteria

- Dependency resolution uses `wide` 1.4.0; workspace MSRV is raised only if
  `wide` requires it, and only to the minimum needed.
- Production NCC refine path uses a single fused `wide` pass computing all five
  NCC sums (no integral image), with the `≤ 1.0` variance reject floor.
- Legacy two-pass NCC is test-only.
- Golden sequence outputs do not regress.
- Repeated-grid ambiguity rejection still holds.
- `Duplicate`, `DimensionMismatch`, `NoMatch`, `ReverseDirection`, and
  verifier failure invariants remain unchanged.
- `template_ncc_us` p50 improves on NCC-heavy benchmark scenarios, or the
  compare report documents why the expected improvement did not appear.
- `bench-results/compare/2026-05-27-p3-fast-ncc-compare.md` is generated with
  frontmatter and committed.
- Raw before/after JSONL exists locally under
  `bench-results/runs/p3-fast-ncc/`.

## Risks and Mitigations

| Risk | Mitigation |
|---|---|
| Floating-point differences alter candidate ordering | Keep legacy NCC under tests, compare with tolerance, and verify final golden outputs. |
| Fused pass changes low-variance outcomes vs legacy | Use legacy's `≤ 1.0` reject floor; add a low-variance equivalence test plus golden fixtures. |
| `wide` speedup is limited by row load overhead or memory bandwidth | Bench first implementation; only optimize load strategy if `template_ncc_us` remains high. |
| Build accidentally depends on host-only CPU features | Do not require `target-cpu=native`; document that distributed binaries use baseline target features. |
| Matcher signatures churn too much | Limit signature changes to template refine helpers and keep public crate API unchanged. |

## Out of Scope

- True image pyramid (P5).
- Axis-locked fast path (P4).
- HNSW/indexed feature fallback (P6).
- Phase correlation experiments (P8).
- Capture Y-plane direct input (P9).
- Runtime CPU feature dispatch or target-specific handwritten intrinsics.
