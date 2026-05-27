# P3 — Fast NCC with Integral Stats and `wide` SIMD (Design)

> Roadmap source: `docs/stitching-rollshot-optimizations-2.md` §5 (P3). This
> spec is **live** for the duration of the P3 workflow. It is the source of
> truth until the branch lands, after which it becomes a frozen snapshot.

## Goal

Reduce the cost of the full-resolution template NCC refine stage without
changing matcher semantics.

Today `ncc_score_shifted` scans the clipped overlap twice for every refinement
offset: once to compute means and once to compute covariance/variance. P3
changes this to fast normalized cross-correlation:

- integral images provide O(1) `sum` and `sum_sq` for each frame window;
- `wide::f32x8` computes the remaining cross term (`sum_xy`) over contiguous
  row spans;
- final NCC is computed from those sums.

This is a performance change only. Candidate ranking may differ by small
floating-point tolerance, but verifier-facing outcomes and golden outputs must
not regress.

## Decisions

- Raise workspace MSRV from Rust 1.85 to Rust 1.89.
- Add `wide = "1.4.0"` as a normal `rollshot-core` dependency.
- Do not add a SIMD feature flag. The production path uses `wide` by default.
- Do not require `RUSTFLAGS="-C target-cpu=native"` or non-baseline target
  features for release builds.
- Keep the old two-pass NCC implementation only under `#[cfg(test)]` for
  equivalence tests.
- Store integral images on `PreparedFrame` via lazy cache, not as ad hoc values
  threaded beside gray buffers.

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
  coarse samples, and lazy edge projections. It does not yet cache integral
  images.
- `rollshot-core` currently depends on `image`, `imageproc`, and `rayon`. It has
  no `[features]` section.

## Platform and Dependency Notes

`wide` 1.4.0 is acceptable for P3 after raising MSRV to 1.89. Its documented
platform model is:

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

### `IntegralImage`

Add a matcher-internal summed-area table:

```rust
struct IntegralImage {
    width: usize,
    height: usize,
    sum: Vec<f64>,
    sum_sq: Vec<f64>,
}
```

The buffer shape is `(width + 1) * (height + 1)` so rectangle queries do not
need edge branches. `sum` and `sum_sq` use `f64` to avoid unnecessary
accumulation error on larger frames.

APIs:

```rust
impl IntegralImage {
    fn from_gray_f32(gray: &[f32], width: usize, height: usize) -> Self;
    fn rect_sum(&self, x: usize, y: usize, w: usize, h: usize) -> f64;
    fn rect_sum_sq(&self, x: usize, y: usize, w: usize, h: usize) -> f64;
}
```

`IntegralImage` can live in `matcher.rs` for the first P3 patch. If the matcher
file becomes harder to reason about during implementation, move NCC-specific
helpers into a private `ncc.rs` module. Do not widen public API solely for this.

### `PreparedFrame`

Extend `PreparedFrame` with a lazy integral cache:

```rust
integral: OnceLock<IntegralImage>,
```

Add a private accessor:

```rust
fn integral(&self) -> &IntegralImage {
    self.integral.get_or_init(|| {
        IntegralImage::from_gray_f32(&self.gray, self.width as usize, self.height as usize)
    })
}
```

The integral is lazy so duplicate frames and match paths that never reach
template refine do not pay for it. The first template refine for a frame builds
the integral under the existing `template_ncc_us` timing. That attribution is
acceptable because the cache exists specifically to accelerate NCC.

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

- `prev.gray()` and `curr.gray()` for `sum_xy`;
- `prev.integral()` and `curr.integral()` for `sum`, `sum_sq`;
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
5. Compute:
   - `n = rect_w * rect_h`
   - `sum_prev`, `sum_prev_sq` from `prev.integral()`
   - `sum_curr`, `sum_curr_sq` from `curr.integral()`
   - `sum_xy` with the `wide` dot scan
6. Compute NCC:

```rust
let numerator = sum_xy - (sum_prev * sum_curr / n);
let prev_var = sum_prev_sq - (sum_prev * sum_prev / n);
let curr_var = sum_curr_sq - (sum_curr * sum_curr / n);

if prev_var <= 1e-9 || curr_var <= 1e-9 {
    return f32::MIN;
}

(numerator / (prev_var * curr_var).sqrt()) as f32
```

Use a small positive variance epsilon for numerical stability. The production
function should return `f32::MIN` on unusable windows, matching current
`ncc_score_shifted` behavior for zero/near-zero variance.

## `wide` Cross Term

Implement the cross term as a row-contiguous dot product:

```rust
fn dot_wide_f32(
    prev_gray: &[f32],
    curr_gray: &[f32],
    width: usize,
    prev_rect: Region,
    curr_rect: Region,
) -> f64
```

For each row:

- process chunks of 8 pixels with `wide::f32x8`;
- accumulate into one or more `f32x8` lanes;
- finish the row tail with scalar math;
- reduce SIMD lanes to `f64` and add the scalar tail as `f64`.

Do not use `unsafe`. `wide::f32x8::from([f32; 8])` is acceptable for the first
implementation. If benchmark results show load construction overhead is
material, a follow-up can revisit layout/load strategy.

The dot helper is intentionally private. P3 does not create a general SIMD
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

The integral build time is counted in `template_ncc_us` because it is demanded
by template refine and amortized across offsets for a frame pair.

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
must explain whether integral construction, rayon overhead, memory bandwidth,
or scenario mix likely dominated.

## Tests

Add focused unit tests near matcher tests:

- `integral_rect_sum_matches_naive_sum`
- `integral_rect_sum_sq_matches_naive_sum_sq`
- `fast_ncc_matches_legacy_ncc_for_random_rects`
- `fast_ncc_preserves_best_offset_on_synthetic_scroll`
- `fast_ncc_handles_constant_windows_as_no_score`
- `repeated_grid_is_still_rejected_by_second_best_margin`

`fast_ncc_matches_legacy_ncc_for_random_rects` should compare scores with a
tolerance, not exact equality. A starting tolerance of `1e-4` is reasonable
because legacy NCC accumulates in `f32` while fast NCC uses `f64` for sums and
may use different SIMD reduction order for `sum_xy`.

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

- Workspace MSRV is Rust 1.89 and dependency resolution uses `wide` 1.4.0.
- Production NCC refine path uses integral stats plus `wide` cross term.
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
| Integral cache increases memory | Build lazily; one integral per `PreparedFrame`; measure peak RSS in benchmark report. |
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
