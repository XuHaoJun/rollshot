# Rollshot Overlap-and-Overwrite Stitch Topology Design (v0.3)

Date: 2026-05-22

## Scope

This spec defines rollshot v0.3: a stitch-topology rework that **replaces**
v0.2.1's static region mask (commit `1b16e8de`). v0.3 reverts the v0.2.1
detector and adopts an overlap-and-overwrite slicing topology inspired by the
snow-shot project's `app-scroll-screenshot-service` crate.

v0.2's `LinearCanvas` appends the minimal new slice (`slice_px = motion delta`)
with zero overlap onto the previous canvas tail. That topology paints any
per-frame trailing-edge artifact — sticky footer, sticky header (in scroll-up),
1 px browser-chrome decorative border — into the canvas at every slice
boundary, producing visible repetition every `slice_px` rows down the stitched
image. v0.2.1 attempted to mitigate this with a `StaticRegionDetector` that
finds anchored edge bands and overwrites them with a sampled background color.
That worked for thick uniform-color sticky UI (real sticky header / footer)
but failed for thin decorative borders, because `bg_color` was sampled from
inside the band — replacing a 1 px gray line with the same gray.

v0.3 abandons detection entirely. Each new slice is widened to
`max(H/2, slice_px)` rows / cols (snow-shot's formula) and pasted **back into**
the existing canvas by `overlap_size = max(0, H/2 - slice_px)` pixels, so it
overwrites the previous slice's trailing portion. The next slice then
overwrites the current slice's trailing portion. Only the most recently
appended slice's trailing pixels survive in the canvas — exactly one copy of
any per-frame trailing-edge artifact remains, located at the canvas's leading
edge (and overwritten in turn by the next append).

Matcher, verifier, axis-lock, duplicate detection, AKAZE fallback, capture
backends, and CLI are untouched.

## Goals

- Eliminate per-slice-boundary repetition of sticky horizontal bands and
  decorative edge lines for the common real-world web layouts (Gmail-style,
  Notion-style, X-style: header + solid sidebar + footer).
- Solve the user-reported 1 px browser chrome border duplication directly,
  without configuration.
- Delete v0.2.1's `static_region.rs` module (~810 lines), its integration test
  file (~326 lines), and the `LinearCanvas::append` mask parameter — net code
  reduction of ~1100 lines.
- Keep `LinearCanvas::append` public signature identical to v0.2 (no `mask`
  parameter), so any downstream caller written against v0.2 works unchanged.
- Preserve v0.2 byte-identical output on pure-scroll fixtures (algebraic
  equivalence, gated by a regression test).
- Preserve v0.2's bidirectional axis support (Bottom + Top within the same
  vertical axis, Right + Left within the horizontal axis).

## Non-Goals

- No crop mode (cutting the final trailing-edge row from the output entirely).
  Deferred to v0.4.
- No detection-based mask for patterned / textured sticky sidebars. Documented
  as a known limitation; deferred to a future spec only if real-world demand
  surfaces.
- No `frame_margins` static pre-crop configuration. Deferred; v0.3 alone
  should solve the common cases without configuration.
- No semantic mask via OCR / DOM / accessibility tree.
- No changes to motion estimation, verifier, axis classification, AKAZE
  fallback, capture, or CLI.
- No new public types or configuration knobs. v0.3 is strictly subtractive on
  the public API (relative to v0.2.1).

## What v0.3 Naturally Handles

For vertical scroll-down (symmetric for scroll-up, horizontal-right,
horizontal-left):

| Sticky element | v0.3 behavior |
| --- | --- |
| Top horizontal band (header) | Frame 1's header preserved at canvas top; subsequent slices do not include header rows, no repetition. |
| Bottom horizontal band (footer, 1 px decorative border) | Each new slice's overlap overwrites the previous slice's footer; only the most recent footer survives at canvas bottom. |
| Solid-color left or right sidebar | Continuous column down the canvas; overlap writes the same solid color over the same solid color — visually clean, no artifact. |
| Top-anchored sidebar icon (icon at small frame-local y) | Lives in the upper portion of each frame, never enters the slice in scroll-down — frame 1's icon preserved at canvas top. |
| Bottom-anchored sidebar icon (icon at large frame-local y) | Enters every slice but each new slice's overlap overwrites the previous icon — only the most recent appears at canvas bottom. |
| Header / footer in scroll-up (using `prepend_top`) | Symmetric: most recent prepend overwrites previous prepend's overlap region, only the most recent header survives at canvas top. |

## What v0.3 Does NOT Handle

These are documented limitations, not bugs:

| Sticky element | Why v0.3 cannot help |
| --- | --- |
| Patterned / textured sticky sidebar where pixels are a function of frame-local y (e.g. v0.2.1 test fixture's `(y/7) % 2` stripe) | Each frame's slice writes its own pattern offset; pattern transitions across the overlap boundary by `delta` rows (not aligned with pattern period). Same number of seams as v0.2, just relocated to overlap boundaries. |
| Middle-anchored sticky element inside a sidebar (icon near frame-local `y = H/2`) | Sits exactly at the overlap boundary; frame 1's copy is preserved below the boundary while subsequent frames place their copy above the boundary. Duplicates. |

Both cases are rare in real web design — production sidebars use solid
backgrounds with top- or bottom-anchored navigation. The v0.2.1 patterned
sidebar test fixture was an artificial worst case for the detector, not a
representative use case.

## Architecture

v0.3 modifies a single internal surface — `LinearCanvas::append_*` /
`prepend_*` — and removes everything v0.2.1 added.

### Module changes

```text
Deleted:
  crates/rollshot-core/src/static_region.rs        (~813 lines)
  crates/rollshot-core/tests/static_region.rs      (~326 lines)

Reverted to v0.2:
  crates/rollshot-core/src/lib.rs                  (drop static_region re-exports)
  crates/rollshot-core/src/types.rs                (drop StitchConfig::static_region field)
  crates/rollshot-core/src/stitcher.rs             (drop static_detector field + observe/mask plumbing)
  crates/rollshot-core/src/canvas.rs               (drop mask parameter from append helpers;
                                                    apply_static_mask deleted)

Modified for v0.3:
  crates/rollshot-core/src/canvas.rs               (overlap-and-overwrite logic in 4 helpers)

New for v0.3:
  crates/rollshot-core/tests/overlap_topology.rs   (integration tests replacing static_region.rs)

Retained:
  crates/rollshot-core/tests/common/mod.rs         (paint_sticky_* helpers stay; reused by new tests)
```

### Public API removed

```rust
// All from rollshot_core's public surface:
StaticMask
StickyBand
StaticRegionConfig
StitchConfig::static_region          // field
```

### Public API restored to v0.2

```rust
impl LinearCanvas {
    pub fn append(
        &mut self,
        direction: AppendDirection,
        frame: &RgbaImage,
        slice_px: u32,
    ) -> Result<u32, CanvasAppendError>;
}
```

The `mask: Option<&StaticMask>` parameter introduced by v0.2.1 is gone.
`Stitcher`, `StitchOutcome`, `StitchStats`, `MotionEstimate`, and the
`MotionCandidate` / matcher / verifier types are unchanged.

### Data flow

```text
RgbaImage frame stream
-> duplicate detection
-> AutoHybrid motion estimation
-> generic overlap verification
-> axis detection or axis-lock validation
-> LinearCanvas::append(direction, &frame, slice_px)
     └─ NEW: overlap-and-overwrite slice computation (this spec)
-> stitched RgbaImage output
```

The stitcher's `push_frame` flow is byte-equivalent to v0.2; only the
`canvas.append` call site loses its `mask` argument.

## Canvas Overlap Algorithm

Snow-shot's formula: `overlap_size = max(0, scroll_side_size / 2 - |delta|)`.
Total slice height (or width) = `slice_px + overlap_size = max(H/2, slice_px)`.

### Append Bottom (vertical scroll-down)

```text
inputs: frame (W × H), slice_px (= |motion.dy|)

clamp:  slice_px = slice_px.min(H)

overlap_size = (H / 2).saturating_sub(slice_px)
total_slice  = (slice_px + overlap_size).min(H)

slice = frame.view(0, H - total_slice, W, total_slice).to_image()

new_height = canvas.height() + slice_px
paste_y    = canvas.height() - overlap_size

combined = RgbaImage::new(W, new_height)
combined.copy_from(canvas.image(), 0, 0)
combined.copy_from(&slice, 0, paste_y)

canvas.image = combined
return slice_px
```

### Prepend Top (vertical scroll-up)

```text
inputs: frame (W × H), slice_px (= |motion.dy|, direction Top)

clamp:  slice_px = slice_px.min(H)

overlap_size = (H / 2).saturating_sub(slice_px)
total_slice  = (slice_px + overlap_size).min(H)

slice = frame.view(0, 0, W, total_slice).to_image()

new_height = canvas.height() + slice_px

combined = RgbaImage::new(W, new_height)
combined.copy_from(&slice, 0, 0)
combined.copy_from(
    canvas.image().view(0, overlap_size, W, canvas.height() - overlap_size),
    0,
    total_slice,
)

canvas.image = combined
return slice_px
```

The existing canvas's top `overlap_size` rows are dropped (replaced by the new
slice's overlap tail). Net canvas growth is `slice_px`.

### Append Right / Prepend Left

Symmetric to append_bottom / prepend_top with x/y dimensions swapped; the
formula uses `W/2` instead of `H/2`.

```text
# Append Right
overlap_size = (W / 2).saturating_sub(slice_px)
total_slice  = (slice_px + overlap_size).min(W)
slice = frame.view(W - total_slice, 0, total_slice, H).to_image()
paste_x = canvas.width() - overlap_size
# combined = new(canvas.width + slice_px, H); copy canvas, paste slice at paste_x

# Prepend Left
overlap_size = (W / 2).saturating_sub(slice_px)
total_slice  = (slice_px + overlap_size).min(W)
slice = frame.view(0, 0, total_slice, H).to_image()
# combined = new(canvas.width + slice_px, H); paste slice at 0; paste canvas (skipping its
# left overlap_size cols) at total_slice
```

### Walk-through

`H = 200`, `slice_px = 40`, canvas already holds frame 1 (height = 200).

```text
overlap_size = 100 - 40 = 60
total_slice  = 100
slice        = frame 2 rows [100..200)        # 100 rows from frame 2's bottom
paste_y      = 200 - 60 = 140
new_height   = 240

Resulting canvas:
  y =   0..139  frame 1 (preserved)
  y = 140..199  slice rows  0..59  = frame 2 rows 100..159  (overlap; overwrites frame 1)
  y = 200..239  slice rows 60..99  = frame 2 rows 160..199  (new content)
```

For the user's 1 px browser bottom border (at frame row 199):
- Frame 1's gray row was at canvas y=199 — overwritten by slice row 59 (frame 2 row 159, page content, not gray).
- Frame 2's gray row lands at canvas y=239 — will be overwritten when frame 3 arrives.
- Only the very last appended slice's gray row survives in the final canvas, at the canvas's leading edge.

## Edge Cases

1. **First append** (canvas height equals frame height). `paste_y = H - (H/2 - delta) = H/2 + delta > 0`. No underflow.
2. **Motion > H/2**. `overlap_size = 0`, `total_slice = slice_px`. Behavior is byte-identical to v0.2 minimal-slice append.
3. **Tiny motion (delta = 1)**. `overlap_size = H/2 - 1`, `total_slice = H/2`. Each frame contributes 1 new row and overwrites `H/2 - 1` previously-painted rows. Correct but the per-frame copy cost dominates throughput for very fine-grained scrolls.
4. **Slice larger than frame**. Existing v0.2 clamp on `slice_px` is preserved; v0.3 adds an analogous clamp on `total_slice`. The `min(frame.height())` saturating safeguard handles the degenerate case even though it should not arise from the formula given `slice_px ≤ H`.
5. **Same-axis direction flip** (scroll-down then scroll-up, locked vertical axis). v0.2 supports this. v0.3 supports it too: each direction maintains its own overlap region at the corresponding canvas edge. Axis-lock logic in `stitcher.rs` is unchanged.
6. **Cross-axis motion**. Returns `StitchOutcome::AxisChanged` as in v0.2. v0.3 has no interaction with this path.

## Memory and Performance

Per-append RAM (transient peak above the canvas baseline):

| Direction | v0.2 | v0.3 |
| --- | --- | --- |
| Slice buffer | `slice_px · W · 4` | `max(H/2, slice_px) · W · 4` |
| Combined buffer | `(canvas.height + slice_px) · W · 4` | same |

For `W = 1200, H = 900, slice_px = 40`: slice grows from 192 KB to 2.16 MB
transient; ~2 MB peak overhead per append. The final canvas size and per-frame
CPU work are dominated by the combined buffer copy, which is unchanged
asymptotically. The extra `(H/2 - delta) · W` pixel copy per frame
(~520 KB for the parameters above) costs <0.1 ms at typical desktop memory
bandwidth — far below the matcher / verifier per-frame cost.

The existing `large_pair_stays_within_structural_search_budget` performance
test in `matcher.rs` is unaffected (matcher unchanged).

## Backwards Compatibility

v0.2.1 is on `main` (commit `1b16e8de`). v0.3 is a breaking change relative to
v0.2.1: the public types `StaticMask`, `StickyBand`, `StaticRegionConfig` and
the `StitchConfig::static_region` field are removed. Rollshot is pre-1.0 with
no stability promise on these surfaces; the CHANGELOG / release notes will
call this out.

Relative to v0.2 (pre-`1b16e8de`), v0.3's public API is **identical**. Any
caller written against v0.2 compiles and runs unchanged against v0.3.

## Testing Strategy

### Test plan summary

| Scenario | Expected behavior | Test name |
| --- | --- | --- |
| Pure scroll, no sticky | byte-identical to v0.2 minimal-slice | `pure_scroll_byte_identical_to_v0_2_minimal_slice` |
| Sticky header (scroll-down) | only frame 1's header at canvas top | `sticky_header_appears_only_at_canvas_top` |
| Sticky footer | only at canvas bottom | `sticky_footer_only_at_canvas_bottom` |
| 1 px decorative border | only at canvas bottom | `decorative_1px_bottom_border_only_at_canvas_bottom` |
| Solid sidebar | continuous column, no artifact | `solid_sidebar_renders_as_continuous_column` |
| Top-anchored sidebar icon | preserved from frame 1 | `top_anchored_sidebar_icon_preserved_from_first_frame` |
| Bottom-anchored sidebar icon | only at canvas bottom | `bottom_anchored_sidebar_icon_only_at_canvas_bottom` |
| Sticky header in scroll-up | only at canvas top | `sticky_header_after_scroll_up_appears_only_once` |
| Horizontal sticky top band | symmetric to vertical | `horizontal_scroll_with_sticky_top_band` |
| Horizontal sticky bottom band | symmetric to vertical | `horizontal_scroll_with_sticky_bottom_band` |
| Motion > H/2 | falls back to v0.2 behavior | `motion_larger_than_half_frame_falls_back_to_v0_2_behavior` |
| Bidirectional scroll | both edges consistent | `bidirectional_scroll_down_then_up_canvas_consistent` |
| First frame preserved | verbatim | `first_frame_preserved_verbatim` |

### Canvas unit tests (`crates/rollshot-core/src/canvas.rs`)

The v0.2 test set is restored (mask parameter removed). New v0.3-specific
tests are added:

```text
- overlap_bottom_paste_position_is_canvas_height_minus_overlap
- overlap_top_prepend_skips_overlap_rows_of_existing_canvas
- overlap_right_paste_position_is_canvas_width_minus_overlap
- overlap_left_prepend_skips_overlap_cols_of_existing_canvas
- net_growth_equals_slice_px_in_all_directions
- large_motion_uses_zero_overlap
- tiny_motion_uses_h_over_2_minus_one_overlap
- axis_lock_still_enforced
- dimension_mismatch_still_reported
- zero_slice_px_still_rejected
- slice_clamped_to_frame_size
```

### Integration tests (new file `crates/rollshot-core/tests/overlap_topology.rs`)

Replaces the deleted `tests/static_region.rs`. Uses the existing
`tests/common/mod.rs` paint helpers (`paint_sticky_header`,
`paint_sticky_footer`, `paint_sticky_sidebar`, `paint_sticky_horizontal_band`)
plus a new helper or two for "anchored icon" fixtures.

### Pure-scroll regression gate

```text
- pure_scroll_byte_identical_to_v0_2_minimal_slice
```

This test asserts pixel-for-pixel equality of v0.3 output against a stored
golden image captured at v0.2 (pre-`1b16e8de`). For pure-scroll fixtures with
no per-frame variation and no sticky UI, overlap-and-overwrite is algebraically
equivalent to minimal-slice append: every overlap pixel that v0.3 overwrites
was the same source-canvas pixel that v0.2 had already placed there. Any drift
from byte-identical indicates a bug in the new slice math.

### Deleted tests

```text
- detector_disabled_via_config_reproduces_legacy_pixel_for_pixel   # config flag gone
- no_sticky_baseline_output_byte_identical_to_disabled_config       # config flag gone
- detector_returns_none_*                                           # detector gone
- pure_scroll_input_locks_with_all_none_bands                       # detector gone
- ... and all other static_region.rs / tests/static_region.rs tests
```

### Performance

No new perf gate; the algorithm's per-frame cost is the same order of
magnitude as v0.2's. The matcher / verifier still dominate.

## Risks

| Risk | Severity | Mitigation |
| --- | --- | --- |
| Pure-scroll output diverges from v0.2 (breaks downstream goldens) | High | `pure_scroll_byte_identical_to_v0_2_minimal_slice` regression gate; algebraic equivalence proof in spec walk-through. |
| Patterned sticky sidebar still shows pattern seams at overlap boundaries | Low | Documented in "What v0.3 Does NOT Handle"; rare in real web design; deferred to a future spec if user demand surfaces. |
| Middle-anchored sticky icon inside sidebar duplicates | Low | Documented; rare. |
| Per-append peak RAM grows from `delta·W·4` to `(H/2)·W·4` | Low | <3 MB transient at typical viewport sizes; acceptable for a desktop app. |
| Motion > H/2 falls back to v0.2 behavior (visible seam for sticky footer in fast scrolls) | Low | Documented; fast wheel / PgDn scrolls rarely exceed half a viewport. |
| Same-axis direction flip (down→up) leaves the post-flip leading edge looking different from the pre-flip edge | Medium | New `bidirectional_scroll_down_then_up_canvas_consistent` test gates this. Each direction independently applies its overlap at the corresponding canvas edge; axis-lock unchanged. |
| Removing `StaticMask` / `StickyBand` / `StaticRegionConfig` is a breaking change relative to v0.2.1 | Low | Rollshot is pre-1.0; CHANGELOG flags the removal; v0.3 is API-equivalent to v0.2 so consumers on v0.2 see zero diff. |

## Acceptance Criteria

```
[ ] commit 1b16e8de's content is fully reverted (either via git revert or a
    single equivalent PR that removes the same surface)
[ ] crates/rollshot-core/src/static_region.rs deleted
[ ] crates/rollshot-core/tests/static_region.rs deleted
[ ] StaticMask / StickyBand / StaticRegionConfig types removed from the
    rollshot_core public API
[ ] StitchConfig::static_region field removed
[ ] LinearCanvas::append signature reverts to v0.2 (no mask parameter)
[ ] Stitcher::static_detector field removed; observe / mask plumbing removed
    from push_frame
[ ] LinearCanvas::append_bottom uses overlap-and-overwrite per the algorithm
    in this spec
[ ] LinearCanvas::prepend_top uses overlap-and-overwrite (symmetric)
[ ] LinearCanvas::append_right uses overlap-and-overwrite (symmetric)
[ ] LinearCanvas::prepend_left uses overlap-and-overwrite (symmetric)
[ ] crates/rollshot-core/tests/overlap_topology.rs covers every test in the
    "Test plan summary" table
[ ] pure_scroll_byte_identical_to_v0_2_minimal_slice passes
[ ] cargo test --workspace passes
[ ] cargo fmt --check passes
[ ] cargo clippy --workspace --all-targets -- -D warnings passes
[ ] docs/rollshot_mvp_design.md updated:
    - §3.2.1 ("Static Region Mask") replaced with overlap-and-overwrite topology
    - §20 risk table row updated to point at v0.3
```

## Future Work

- **v0.4 crop mode**: cut the final trailing-edge `1..2` rows of the canvas at
  export time so even the most recent slice's decorative border is removed.
  Requires a per-direction "trim tail" configuration on `StitchConfig`.
- **v0.4 frame margins**: optional `frame_margins: { top, bottom, left, right }`
  pre-crop applied before all stitcher processing. Useful when the user knows
  ahead of time which edge rows are window chrome.
- **v0.4+ patterned-sidebar mask**: if real demand surfaces, reintroduce a
  narrower detector specifically for vertical strips with internal pattern.
  Would coexist with overlap topology, not replace it.
- **v0.5+ semantic mask sources**: OCR / DOM / accessibility tree to handle
  cases pixel-only analysis cannot disambiguate.
