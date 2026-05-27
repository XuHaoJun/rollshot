# P1 StripCanvas Design

Date: 2026-05-27

## Goal

Replace `rollshot-core`'s eager, growing `LinearCanvas` with a strip-backed
canvas that preserves current stitching output semantics while making append
cost proportional to the incoming slice, not the already-stitched image size.

This is the first optimization after the benchmark harness. It targets the P1
item in `docs/stitching-rollshot-optimizations-2.md`: long screenshots should
stop getting slower and memory-spikier solely because the stitched canvas has
grown large.

## Decision

Implement `StripCanvas` as the primary canvas implementation. Do not add a
feature flag and do not keep a runtime switch between `LinearCanvas` and
`StripCanvas`.

The implementation may change `Stitcher::full_image` from:

```rust
pub fn full_image(&self) -> Option<&RgbaImage>
```

to:

```rust
pub fn full_image(&mut self) -> Option<&RgbaImage>
```

The mutable receiver is intentional: composing strips into a full image is a
lazy cache fill. This is preferable to hiding mutation behind `RefCell` just to
preserve an immutable API shape.

## Non-goals

- No `PreparedFrame` cache.
- No NCC integral-image or SIMD work.
- No axis-locked matcher fast path.
- No changes to duplicate detection, matcher candidate ordering, verifier
  thresholds, axis classification, or reverse-direction policy.
- No bidirectional canvas expansion beyond preserving the current top/left
  prepend behavior.
- No CI benchmark gate.

## Current Context

`LinearCanvas` owns one `RgbaImage`. Each append allocates a larger `RgbaImage`,
copies the existing canvas, then copies the incoming overlap-and-new-content
slice. This preserves the desired overlap-and-overwrite topology, but append
cost and transient memory grow with the accumulated canvas.

The benchmark harness from `docs/superpowers/specs/2026-05-26-benchmark-harness-design.md`
is complete. A P1 baseline was captured before this spec:

- Baseline commit: `f404e61`
- Raw JSONL: `bench-results/2026-05-27-p1-strip-canvas-before-f404e61.jsonl`
- Summary: `bench-results/2026-05-27-p1-strip-canvas-before-f404e61.summary.md`
- Scenarios: `long_vertical_text`, `long_sticky_header`, `long_vertical_jitter`
- Repeats: 3

Key baseline values:

| scenario | p50 total | p95 total | p50 append | peak RSS delta |
|---|---:|---:|---:|---:|
| `long_vertical_text` | 21,793 us | 27,849 us | 7,582 us | 71,784 kB |
| `long_vertical_jitter` | 20,897 us | 28,697 us | 6,438 us | 71,760 kB |
| `long_sticky_header` | 14,905 us | 20,636 us | 0 us | 56,036 kB |

The baseline was captured before `bc7f662`, which only fixed Cargo's trailing
`--bench` CLI compatibility for the harness and does not change stitching
behavior.

## Architecture

Introduce a strip-backed canvas in `crates/rollshot-core/src/canvas.rs`:

```rust
pub struct StripCanvas {
    axis: Option<ScrollAxis>,
    logical_width: u32,
    logical_height: u32,
    strips: VecDeque<CanvasStrip>,
    composed_cache: Option<RgbaImage>,
    last_append_copied_bytes: u64,
}

struct CanvasStrip {
    image: RgbaImage,
    x: i64,
    y: i64,
    slice_px: u32,
    overlap_px: u32,
}
```

The first frame is stored as the first strip at `(0, 0)`. Later appends store
only the cropped incoming slice that current `LinearCanvas` would paste into
the combined image.

`logical_width` and `logical_height` represent the externally visible stitched
image dimensions. `full_image()` composes all strips into a cached `RgbaImage`
with those dimensions. Appending or prepending invalidates the cache.

`strips` are ordered by paste time, not by spatial position. Composition is
last-write-wins in that order. This matters for top/left prepends: after
existing strips are shifted, the new top/left crop must still be pasted after
the older strips so it overwrites the intentional overlap region.

Use `VecDeque` rather than `Vec` because current behavior supports top/left
prepend as well as bottom/right append. This keeps the implementation honest
without designing a future bidirectional scrolling mode.

## Append Semantics

The new canvas must preserve the existing overlap-and-overwrite topology
byte-for-byte.

For `Bottom`, the current geometry is:

```text
overlap_px = max(0, frame.height / 2 - slice_px)
total_slice = min(frame.height, slice_px + overlap_px)
crop = frame rows [frame.height - total_slice, frame.height)
paste_y = old_logical_height - overlap_px
new_logical_height = old_logical_height + slice_px
```

`StripCanvas` stores that crop as a strip at `(0, paste_y)`.

For `Top`, current behavior crops from the top of the incoming frame and drops
the overwritten leading overlap from the old canvas by shifting retained old
content down. The strip representation should express the same final positions:

- Add the incoming top crop at `y = 0`.
- Shift existing strips down by `slice_px`.
- Increase `logical_height` by `slice_px`.
- Store the new crop after existing strips in paste order so the overlap still
  overwrites old pixels.

For `Right`, mirror `Bottom` horizontally:

- Crop from the right edge.
- Paste at `old_logical_width - overlap_px`.
- Increase `logical_width` by `slice_px`.

For `Left`, mirror `Top` horizontally:

- Add the incoming left crop at `x = 0`.
- Shift existing strips right by `slice_px`.
- Increase `logical_width` by `slice_px`.
- Store the new crop after existing strips in paste order so the overlap still
  overwrites old pixels.

This shifting model is acceptable because normal rollshot captures are
one-directional after axis/direction lock. Top/left prepends remain correct,
but this spec does not optimize repeated bidirectional growth.

## Composition

`full_image()` should lazily compose:

```rust
fn full_image(&mut self) -> Option<&RgbaImage> {
    if self.composed_cache.is_none() {
        let mut out = RgbaImage::new(self.logical_width, self.logical_height);
        for strip in &self.strips {
            overlay_copy(&mut out, &strip.image, strip.x, strip.y);
        }
        self.composed_cache = Some(out);
    }
    self.composed_cache.as_ref()
}
```

`overlay_copy` must copy rows or columns using raw slices, not per-pixel
`put_pixel`, and must clip safely to the logical output bounds.

`into_image` may consume the canvas and compose once without cloning the cached
image. It is fine if `Stitcher` does not expose `into_image` yet.

## Compaction

Storing each appended slice as an immutable strip retains the overlap region
redundantly. Under overlap-and-overwrite, a `Bottom` strip stores
`total_slice = min(frame_h, slice_px + overlap_px)` rows but adds only
`slice_px` net rows. When `slice_px < frame_h/2` (slow scroll), each strip
stores `frame_h/2` rows while netting `slice_px`, so total strip bytes grow to
roughly `frame_h / (2 * slice_px)` times the logical canvas.

For the P1 benchmark scenarios (`frame_h = 700`, `slice_px ≈ 40`) that is about
**8x** the final canvas, which would make peak RSS several times *worse* than
`LinearCanvas`, not better. The overlap cannot be dropped — overwriting it is
what hides sticky headers (this is exactly why `wayscrollshot`, which stores
only net-new rows, has no overwrite semantics). So redundancy is reclaimed by
compaction instead.

When total strip bytes exceed `COMPACT_FACTOR * logical_bytes` (default
`COMPACT_FACTOR = 2`), `StripCanvas` composes all strips into a single base
strip at `(0, 0)` and clears the deque:

```rust
const COMPACT_FACTOR: u64 = 2;

fn compact_if_needed(&mut self) {
    let logical_bytes = self.logical_pixels() * 4;
    let strip_bytes: u64 = self
        .strips
        .iter()
        .map(|s| s.image.as_raw().len() as u64)
        .sum();
    if strip_bytes <= logical_bytes.saturating_mul(COMPACT_FACTOR) {
        return;
    }
    self.compose_if_needed();
    let base = self.composed_cache.take().expect("composed image");
    self.strips.clear();
    self.strips.push_back(CanvasStrip {
        image: base,
        x: 0,
        y: 0,
        slice_px: 0,
        overlap_px: 0,
    });
}
```

`compact_if_needed` runs at the end of every `append`, after cache
invalidation. Properties:

- Resident strip memory is bounded at `~COMPACT_FACTOR * logical`, i.e. about
  the same as `LinearCanvas`'s transient peak — not several times larger.
- Append stays `O(frame_h)` amortized: between compactions the strips grow by
  `(COMPACT_FACTOR - 1) * logical` bytes, each compaction costs `O(logical)`, so
  amortized per-append copy is independent of canvas height. This preserves the
  append-latency win over `LinearCanvas`'s `O(canvas_h)` reallocation.
- The compaction frame itself spikes to an `O(logical)` copy plus a transient
  `~3 * logical` allocation during compose. These spikes get rarer as the canvas
  grows, so `p95_append_us` still improves; `p99`/`max_append_us` may show
  occasional compaction spikes and should be reported as such.

A composed base strip carries `slice_px = 0` / `overlap_px = 0`; those fields are
metadata only and are not read during composition.

## Metrics

Keep the existing metric names so benchmark scripts remain unchanged:

- `canvas_logical_pixels`: `logical_width * logical_height`.
- `canvas_allocated_bytes`: total bytes currently owned by strips plus cached
  composed image if present.
- `append_copied_bytes`: bytes copied during the append operation.

For `StripCanvas`, `append_copied_bytes` should count the incoming crop bytes
stored for the new strip, not the full logical canvas size. This makes the P1
improvement visible in the existing JSONL records.

Because `full_image()` may allocate the composed cache, callers that export or
test output should expect `canvas_allocated_bytes` to increase after a
composition has occurred. Between compactions, `canvas_allocated_bytes` stays
bounded at roughly `COMPACT_FACTOR * logical` (plus the composed cache when
present), rather than growing without limit with the number of strips.

## Stitcher Integration

`Stitcher` should own `Option<StripCanvas>` instead of `Option<LinearCanvas>`.

Only the canvas-facing parts of `Stitcher` should change:

- First frame initializes `StripCanvas`.
- Append calls use the same `direction`, `frame`, and `slice_px` inputs.
- `full_image()` takes `&mut self`.
- `snapshot_canvas_state()` reads `logical_pixels()` and `allocated_bytes()`.

The motion path remains unchanged:

```text
duplicate signature
-> estimate_motion(anchor, frame)
-> rank candidates
-> final PixelOverlapVerifier
-> canvas append
-> update last_good frame/signature only after append success
```

## Tests

Add direct canvas equivalence tests before switching `Stitcher` to
`StripCanvas`. These tests should compare old and new behavior after each
append. If the production `LinearCanvas` type is removed, keep a small
test-only `LegacyLinearCanvas` helper in `canvas.rs`'s test module that contains
the old append geometry exactly:

- bottom append matches legacy output byte-for-byte
- top prepend matches legacy output byte-for-byte
- right append matches legacy output byte-for-byte
- left prepend matches legacy output byte-for-byte
- overlap overwrite matches legacy output byte-for-byte
- repeated `full_image()` calls return stable bytes
- append invalidates the composed cache and subsequent `full_image()` reflects
  the new strip
- `last_append_copied_bytes` for strip append is less than the full logical
  canvas allocation after at least one growth append

Existing integration coverage must continue to pass:

- `crates/rollshot-core/tests/stitcher.rs`
- `crates/rollshot-core/tests/overlap_topology.rs`
- `crates/rollshot-core/tests/golden_fixtures.rs`
- `crates/rollshot-core/tests/metrics_population.rs`

Tests that call `Stitcher::full_image()` must make the stitcher binding mutable.

## Benchmark Verification

Before implementing P1, keep the existing baseline files backed up outside
`target/`.

When verifying P1 after implementation, first look for the baseline JSONL under
repo-root `bench-results/`:

```text
bench-results/2026-05-27-p1-strip-canvas-before-f404e61.jsonl
```

If that file is missing, do not silently skip the benchmark comparison. Stop and
ask the user whether they have a backup copy of the baseline. If they do, have
them restore it into `bench-results/` and run the comparison. Only skip the
before/after comparison if the user explicitly says the backup is lost.

After implementation, run the same scenario set with the fixed one-command
harness:

```bash
rtk cargo bench -p rollshot-core --bench stitch_sequences -- \
  --fixtures long_vertical_text,long_sticky_header,long_vertical_jitter \
  --repeats 3 \
  --out bench-results/2026-05-27-p1-strip-canvas-after.jsonl
```

Then compare:

```bash
rtk python3 scripts/bench/compare.py \
  bench-results/2026-05-27-p1-strip-canvas-before-f404e61.jsonl \
  bench-results/2026-05-27-p1-strip-canvas-after.jsonl
```

The PR should report:

- `p95_append_us`
- `append_copied_bytes`
- `canvas_allocated_bytes`
- `peak_rss_kb_delta`
- `total_us`
- output hash and golden diff fields

If the baseline is explicitly unavailable, still run the after benchmark and
record that the run has no before/after comparison because the baseline backup
was lost.

Expected P1 outcome:

- `append_copied_bytes` no longer scales with final canvas size.
- `p95_append_us` drops substantially on `long_vertical_text` and
  `long_vertical_jitter`.
- Peak RSS stays bounded at roughly `COMPACT_FACTOR * logical` via compaction,
  i.e. comparable to the `LinearCanvas` baseline rather than several times
  larger. A regression to a multiple of the baseline means compaction is not
  firing and is a failure, not an accepted tradeoff.
- `p99`/`max_append_us` may show occasional compaction spikes; report them
  rather than treating them as regressions.
- Output remains byte-identical for golden fixtures.
- Synthetic output hashes remain stable unless the legacy output hash was
  dependent on undefined behavior; any hash drift must be explained.

## Acceptance Criteria

- `StripCanvas` is the only canvas implementation used by `Stitcher`.
- Any legacy canvas code that remains exists only as a test helper for
  byte-equivalence tests.
- The public stitching behavior is unchanged except for `full_image(&mut self)`.
- Existing unit and integration tests pass.
- New legacy-vs-strip tests prove byte-identical append topology before the
  legacy implementation is removed or hidden from production use.
- `StripCanvas` compacts strips so resident memory is bounded at
  `~COMPACT_FACTOR * logical`; peak RSS does not regress to a multiple of the
  `LinearCanvas` baseline. A unit test proves strip+cache bytes stay bounded
  under a slow-scroll (`slice_px < frame_h/2`) append sequence.
- Benchmark after-run is captured and compared against the saved P1 baseline.
  If `bench-results/` does not contain the baseline, the implementer asks for a
  backup before proceeding; comparison is skipped only when the user explicitly
  confirms the backup is lost.
- No P2/P3 matcher preparation or NCC changes are included in the same commit
  series.

## Risks

The main correctness risk is top/left prepend positioning. Bottom/right are the
dominant product path, but existing tests cover all four directions and should
continue to do so.

The main API risk is changing `full_image()` to require `&mut self`. This is
acceptable because `full_image()` now performs lazy work. Tests and app callers
must be updated mechanically.

The main memory risk is overlap redundancy: without compaction, strips retain
`frame_h/2` rows per slow-scroll append and resident memory balloons to several
times the logical canvas (~8x on the P1 fixtures), making peak RSS worse than
`LinearCanvas`. Compaction (above) is therefore part of P1, not a follow-up; the
bounded-memory unit test guards against it regressing.

The remaining measurement nuance is that `canvas_allocated_bytes` still rises
after `full_image()` because the composed cache is retained, and the compaction
frame transiently allocates `~3 * logical`. Benchmark comparisons should focus
on per-frame append metrics and steady-state RSS, and separately note the
compaction/composition spikes.
