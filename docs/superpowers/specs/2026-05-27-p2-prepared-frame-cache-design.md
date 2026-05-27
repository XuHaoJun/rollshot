# P2 — PreparedFrame Cache (Design)

> Roadmap source: `docs/stitching-rollshot-optimizations-2.md` §4 (P2). This spec
> is **live** for the duration of the P2 workflow — it is the source of truth
> until the branch lands, after which it becomes a frozen snapshot.

## Goal

Stop recomputing the `last_good` (prev) frame's derived data on every frame.
Today `estimate_motion` rebuilds `to_grayscale(prev)`, `coarse_samples(prev)`,
and `edge_projection(prev)` on every non-duplicate frame, even though `prev` is
just the previous frame's `curr`. Cache prev's derived data so a successful
append carries it forward.

**This is a pure caching refactor: output must stay byte-identical.** No change
to matcher results, verifier behavior, overlap-and-overwrite semantics, or the
invariants in roadmap §1.2.

## Current state (verified against code, 2026-05-27)

- P1 (`StripCanvas`) is done; `LinearCanvas` no longer exists. There is no
  canvas feature flag.
- P0 harness exists: `StitchMetrics` (incl. `prepare_frame_us`, `coarse_us`),
  criterion bench `stitch_sequences`, `scripts/bench/compare.py`.
- `estimate_motion()` (`crates/rollshot-core/src/matcher.rs:136`) takes
  `prev: &RgbaImage, curr: &RgbaImage` and computes both grayscales inside
  (timed under `prepare_frame_us`). `coarse_candidates` recomputes
  `coarse_samples` for **both** prev and curr; `relaxed_coarse_candidate`
  recomputes coarse a second time on the miss path. `edge_projection_axis`
  rebuilds prev+curr projections per axis.
- Only one production caller: `Stitcher::push_frame_inner`
  (`crates/rollshot-core/src/stitcher.rs:90`). Plus ~7 in-file matcher tests and
  the test-only `estimate_motion_with_budget` wrapper.
- `Stitcher` holds `last_good_frame: Option<RgbaImage>` and
  `last_good_signature: Option<Vec<u8>>`; the duplicate gate computes the curr
  signature, checks it, then `estimate_motion` builds grayscale.
- `rollshot-core` has no `[features]` section today.

## Design

### 1. State consolidation in `Stitcher`

Replace the two prev fields with one:

```rust
// before
last_good_frame: Option<RgbaImage>,
last_good_signature: Option<Vec<u8>>,
// after
last_good: Option<PreparedFrame>,
```

`PreparedFrame` owns the RGBA (canvas append still needs it), the duplicate
signature, and the derived caches. On `Appended`, **move** the curr
`PreparedFrame` into `last_good`. Next frame, prev's gray + coarse are already
built; its projections were built lazily during this frame's match.

`accept_first_frame` builds a `PreparedFrame` from the first frame and stores it
as `last_good` (the first frame's signature/gray/coarse are computed once here).

### 2. `PreparedFrame` shape and build cost

```rust
pub(crate) struct PreparedFrame {
    rgba: RgbaImage,
    width: u32,
    height: u32,
    signature: Vec<u8>,          // duplicate::signature (eager, reused from dup gate)
    gray: Vec<f32>,              // eager
    coarse_dims: (u32, u32),     // (sample_w, sample_h) for COARSE_DOWNSAMPLE_STEP (cheap arithmetic, eager)
    coarse: OnceLock<Vec<f32>>,  // lazy
    proj_v: OnceLock<Vec<f32>>,  // lazy, per searched axis
    proj_h: OnceLock<Vec<f32>>,
}
```

- **Eager**: `gray` (+ `signature` reused from the dup gate, + `coarse_dims`
  which is pure arithmetic). `gray` is used on every non-duplicate frame.
- **Lazy** (`std::sync::OnceLock`): coarse samples and edge projections, built on
  first use and cached. Coarse is lazy (not eager) so its build time stays under
  the `coarse_us` timer rather than leaking into `prepare_frame_us` (see §5);
  projections are lazy so the unused axis is skipped when axis-locked. Laziness
  also sidesteps the `&PreparedFrame` vs `&mut` borrow conflict roadmap §4.4
  flagged.
- `OnceLock<Vec<f32>>` is `Send + Sync`, so `PreparedFrame` and `Stitcher` stay
  `Send + Sync` — no risk to the Tauri `Arc<Mutex<Stitcher>>` state. Parallel
  (`rayon`) regions continue to receive `&[f32]` slices, never the
  `PreparedFrame` itself.

Accessors (private to the matcher module; `rgba`/`signature`/`dimensions`/
constructors are `pub(crate)` for the stitcher):

```rust
impl PreparedFrame {
    fn coarse(&self) -> &[f32] {
        self.coarse.get_or_init(|| coarse_samples(&self.gray, self.width, self.height, COARSE_DOWNSAMPLE_STEP))
    }
    fn projection(&self, axis: SearchAxis) -> &[f32] {
        match axis {
            SearchAxis::Vertical   => self.proj_v.get_or_init(|| edge_projection(&self.gray, self.width, self.height, SearchAxis::Vertical)),
            SearchAxis::Horizontal => self.proj_h.get_or_init(|| edge_projection(&self.gray, self.width, self.height, SearchAxis::Horizontal)),
        }
    }
}
```

`PreparedFrame` lives in `matcher.rs` (it depends on the matcher-private
`SearchAxis` and the builder fns `to_grayscale` / `coarse_samples` /
`coarse_sample_dimensions` / `edge_projection`); co-locating avoids widening the
visibility of those five items. The stitcher imports it via `pub(crate)`.

### 3. Matcher API

```rust
pub(crate) fn estimate_motion(
    prev: &PreparedFrame,
    curr: &PreparedFrame,
    locked_axis: Option<ScrollAxis>,
    last_motion: (i32, i32),
    config: &StitchConfig,
    metrics: &mut StitchMetrics,
) -> MotionSearchOutcome
```

- The early `DimensionMismatch` check uses `prev`/`curr` dims (the stitcher
  already guards dimensions before calling, but the matcher keeps its own check
  for the in-file tests).
- `coarse_candidates` reads `prev.coarse` / `curr.coarse` + `coarse_dims`
  instead of calling `coarse_samples`.
- `template_candidates` reads `prev.gray` / `curr.gray`.
- `edge_projection_candidates` reads `prev.projection(axis)` /
  `curr.projection(axis)`.
- `relaxed_coarse_candidate` reuses the same cached coarse/gray — no second
  `coarse_samples` computation.

`coarse_samples`, `coarse_sample_dimensions`, `to_grayscale`, `edge_projection`
remain as the builders used when constructing a `PreparedFrame`.

### 4. Control flow (dup gate stays cheap)

`push_frame_inner` order:

1. First-frame branch → `accept_first_frame` builds `last_good` PreparedFrame.
2. Dimension check (`last_good.rgba.dimensions()` vs frame).
3. Compute **curr signature only** (`duplicate::signature`).
4. Dup check against `last_good.signature`. On duplicate → return early; **no
   gray/coarse build** (matches roadmap §4.7).
5. Build the full curr `PreparedFrame`, reusing the signature from step 3.
6. `estimate_motion(&last_good, &curr_prepared, ...)`.
7. On `Appended`: `self.last_good = Some(curr_prepared)`. On any non-append
   outcome: leave `last_good` unchanged.

### 5. Metrics

- `prepare_frame_us` (timed in the stitcher around `PreparedFrame` construction)
  now times only curr's grayscale, not prev's + curr's two grayscales. Prev's
  build is amortized to ~0. Coarse build is lazy and stays under `coarse_us`, so
  it does not leak into prepare timing.
- `coarse_us` drops: prev's coarse samples are cached, so `coarse_candidates`
  only builds curr's samples (once, via the lazy `coarse()` accessor) plus the
  MAD scoring it always did.
- These are attribution changes only; outputs are unchanged.

### 6. No feature flag

Consistent with how `StripCanvas` shipped (no surviving flag). Safety net is
equivalence + golden byte-identity, not a flag.

## Testing & verification

Unit tests (mirroring roadmap §4.6):

- `prepared_frame_signature_matches_old_signature`
- `prepared_frame_gray_matches_old_to_grayscale`
- `prepared_frame_coarse_matches_old_coarse_samples`
- `prepared_frame_projection_matches_old_edge_projection`
- `prepared_frame_does_not_update_on_no_match`
- `prepared_frame_updates_only_after_appended`

Existing matcher in-file tests + `estimate_motion_with_budget` updated to build a
`PreparedFrame` (test helper, e.g. `PreparedFrame::from_rgba`).

Equivalence / regression:

- Golden sequence tests stay **byte-identical** (`cargo test`).
- `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`.
- Benchmark before/after:
  `cargo bench -p rollshot-core --bench stitch_sequences -- --out bench-results/runs/p2-prepared-frame/after.jsonl`
  then `python3 scripts/bench/compare.py .../before.jsonl .../after.jsonl`.

## Acceptance criteria (roadmap §4.7 + §14 checklist)

- After a successful append, the next frame does not recompute prev gray / prev
  coarse.
- `NoMatch` leaves `last_good` unchanged (same anchor).
- `Duplicate` does not trigger a full `PreparedFrame` build (signature only).
- `prepare_frame_us` p50 down ~30–50% on the bench sequences.
- Golden outputs byte-identical; all existing tests pass.
- §14 invariants intact: Duplicate not into matcher, DimensionMismatch doesn't
  pollute state, NoMatch doesn't update `last_good`, Appended-only update,
  PixelOverlapVerifier still the final gate, ReverseDirection still rejected,
  sticky-header / repeated-grid / low-texture no regression, append time + peak
  RSS not regressed.

## Out of scope (deferred to later roadmap items)

- `u8` gray buffers (§4.5) — separate PR; may shift scores.
- Integral images / pyramid / SIMD (P3, P5).
- Capture Y-plane / external gray injection (P9) — `PreparedFrame` is structured
  so external gray can be added later, but not built here.
