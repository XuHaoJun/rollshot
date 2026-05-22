# Rollshot Static Region Mask Design (v0.2.1)

Date: 2026-05-22

## Scope

This spec defines rollshot v0.2.1: a focused patch on top of v0.2 that removes
the visual duplication of sticky / fixed UI elements (sticky header, sticky
footer, sticky left/right sidebar) in stitched output.

v0.2 already mitigates sticky regions on the *matcher* side through a content
ROI that excludes the outer 4 % / 12 % / 8 % of each frame, so motion estimates
are not biased by fixed UI. However, the matched slice is still pasted into the
canvas across the *entire* frame width / height, so sticky pixels are appended
on every successful frame and visibly repeat down the stitched long image.

v0.2.1 adds a small, self-contained module that:

1. detects which edge bands of the frame are static (do not move with content),
2. samples a representative background color for each detected band,
3. fills those bands with the background color during canvas append.

The first frame is preserved untouched so a single instance of the sticky UI
remains visible at the top / start of the stitched canvas — both as visual
context and as a marker that mask detection succeeded.

This patch does not touch the matcher, verifier, capture layer, or CLI.

## Goals

- Detect static region bands on all four frame edges:
  `top`, `bottom`, `left`, `right`.
- Cover both `ScrollAxis::Vertical` and `ScrollAxis::Horizontal` symmetrically.
- Make detection a standalone module (`static_region.rs`) that can be unit
  tested without running the full stitcher.
- Apply the resulting mask inside `LinearCanvas::append` so the stitcher only
  needs to plumb it through.
- Preserve the first frame's sticky pixels verbatim.
- Default to enabled, with an opt-out flag for diagnostic builds and to allow
  v0.2 byte-identical reproduction when needed.
- Guarantee byte-identical output to v0.2 on fixtures that contain no sticky
  regions (no regression).

## Non-Goals

- No carry-over of real sidebar pixels from the first frame (that is a possible
  v0.2.2 follow-up; v0.2.1 only fills with a flat background color).
- No semantic mask using OCR / DOM / accessibility tree.
- No handling of translucent or blurred sticky elements; they are treated as
  opaque for detection purposes.
- No handling of self-animating sticky elements (loading spinner, video badge);
  the `max_band_ratio` guard rejects them rather than fitting them.
- No handling of mid-frame floating elements (chat widget, banner). The
  algorithm intentionally only accepts contiguous-from-edge bands.
- No change to canvas output dimensions (cropping sticky bands out instead of
  filling them is deferred).
- No changes to the matcher, verifier, axis lock logic, AKAZE fallback, or
  capture backend.

## Architecture

v0.2.1 adds one new module and modifies two existing surfaces:

```text
New:
  crates/rollshot-core/src/static_region.rs
    pub struct StaticMask
    pub struct StickyBand
    pub struct StaticRegionConfig
    pub(crate) struct StaticRegionDetector

Modified:
  crates/rollshot-core/src/canvas.rs
    LinearCanvas::append gains an Option<&StaticMask> parameter

  crates/rollshot-core/src/stitcher.rs
    Stitcher holds a StaticRegionDetector
    push_frame calls detector.observe after motion verification
    push_frame passes detector.mask() to canvas.append

  crates/rollshot-core/src/types.rs
    StitchConfig gains a static_region: StaticRegionConfig field

  crates/rollshot-core/src/lib.rs
    Re-exports StaticMask, StickyBand, StaticRegionConfig
```

The data flow becomes:

```text
RgbaImage frame stream
-> duplicate detection
-> AutoHybrid motion estimation
-> generic overlap verification
-> axis detection or axis-lock validation
-> StaticRegionDetector.observe  (NEW; observes verified motion only)
-> LinearCanvas::append with detector.mask()  (NEW; mask applied at slice time)
-> stitched RgbaImage output
```

`StaticRegionDetector` is `pub(crate)`. It lives entirely inside the stitcher;
callers interact with it through `StitchConfig::static_region` and observe
results through the existing `StitchOutcome` plus the stitched image.

## Public Types

```rust
/// A contiguous static region anchored to one frame edge.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StickyBand {
    /// Thickness in pixels, measured inward from the anchored edge.
    pub thickness: u32,
    /// RGBA fill color used in place of the masked frame pixels.
    pub bg_color: [u8; 4],
}

/// Sticky bands detected on the four frame edges, expressed in the frame's
/// local coordinate space. A field of `None` means no static region was
/// detected on that edge.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct StaticMask {
    pub top:    Option<StickyBand>,
    pub bottom: Option<StickyBand>,
    pub left:   Option<StickyBand>,
    pub right:  Option<StickyBand>,
}

#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct StaticRegionConfig {
    /// Master switch. When `false`, the stitcher behaves identically to v0.2.
    pub enabled: bool,
    /// Number of verified frame pairs to observe before locking the mask.
    pub min_observations: usize,
    /// Maximum allowed mean-absolute-difference for a row / column to be
    /// considered a static candidate, normalized to [0, 1].
    pub static_mad_threshold: f32,
    /// How much smaller the static MAD must be than the motion-aligned MAD
    /// to accept the line as static, normalized to [0, 1].
    pub motion_margin: f32,
    /// Upper bound on per-edge band thickness, as a ratio of the corresponding
    /// frame dimension. Bands above this are treated as detection failure.
    pub max_band_ratio: f32,
}

impl Default for StaticRegionConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            min_observations: 3,
            static_mad_threshold: 4.0 / 255.0,
            motion_margin: 4.0 / 255.0,
            max_band_ratio: 0.30,
        }
    }
}
```

`StaticRegionDetector` itself is not part of the public API; it is an internal
component of `Stitcher`.

## Detection Algorithm

Detection runs only when motion has been verified for the current frame pair.
For each call:

```text
inputs:  prev, curr (both RgbaImage with identical dimensions)
         dx, dy   (verified motion)

step 1  per-row aggregates:
          row_static[y]  = mean over x of |prev(x, y) - curr(x, y)| / 255
          row_motion[y]  = mean over x of |prev(x + dx, y + dy)
                                          - curr(x, y)| / 255
        (only over (x, y) such that the shifted coordinates are in bounds;
         rows with insufficient overlap are skipped)

step 2  per-col aggregates (symmetric):
          col_static[x]  = mean over y of |prev(x, y) - curr(x, y)| / 255
          col_motion[x]  = mean over y of |prev(x + dx, y + dy)
                                          - curr(x, y)| / 255

step 3  line is "static candidate" iff
          static_score < static_mad_threshold
          AND (motion_score - static_score) > motion_margin

step 4  contiguous-from-edge scan:
          top_this    = scan rows 0, 1, 2, ... until first non-static row
          bottom_this = scan rows H-1, H-2, ... until first non-static row
          left_this   = scan cols 0, 1, 2, ... until first non-static col
          right_this  = scan cols W-1, W-2, ... until first non-static col

step 5  guard rails:
          if top_this    > max_band_ratio * H  -> top_this    = 0
          if bottom_this > max_band_ratio * H  -> bottom_this = 0
          if left_this   > max_band_ratio * W  -> left_this   = 0
          if right_this  > max_band_ratio * W  -> right_this  = 0

step 6  sample bg_color for each non-zero band as the channel-wise median
        of pixels in a thin inner strip of that band on `prev`.

step 7  append (top_this, bottom_this, left_this, right_this, bg colors)
        to the detector's observation buffer.
```

After `min_observations` calls, the detector locks:

```text
final extent for each edge  = median of the per-frame extents on that edge
final bg_color  for each edge = channel-wise median of per-frame colors
                                (only over frames whose extent on that edge
                                 was nonzero; if all zero, edge stays None)

resulting StaticMask is cached. Subsequent observe calls are no-ops.
```

Using median (not mean / min / max) for both extent and color makes the lock
robust to one bad frame slipping into the observation window — typical sources
being a transient loading state, a tooltip, or a brief overlay.

If every edge ends up `None`, the detector reports `Some(StaticMask::default())`
rather than `None`. `Some` of an all-`None` mask is the locked signal "we
checked, there is no static UI here"; `None` means "still observing". The
distinction matters for `LinearCanvas::append`, which treats a `None` mask the
same as v0.2 behavior either way, but lets test cases assert that detection
actually ran.

## Mask Application

`LinearCanvas::append` becomes:

```rust
impl LinearCanvas {
    pub fn append(
        &mut self,
        direction: AppendDirection,
        frame: &RgbaImage,
        slice_px: u32,
        mask: Option<&StaticMask>,
    ) -> Result<u32, CanvasAppendError>;
}
```

The four internal helpers (`append_bottom`, `prepend_top`, `append_right`,
`prepend_left`) gain the same `mask` parameter.

After taking the slice (the existing `frame.view(...)` step), each pixel in
the slice is rewritten according to the mask, *using the pixel's coordinates
in the original `frame`'s local space* (not the slice-local space):

```text
let (x, y) = pixel coordinates inside `frame` (W = frame.width, H = frame.height)

# top/bottom take precedence over left/right at corners

if mask.top    is Some(band) and y < band.thickness        -> band.bg_color
else if mask.bottom is Some(band) and y >= H - band.thickness -> band.bg_color
else if mask.left   is Some(band) and x < band.thickness      -> band.bg_color
else if mask.right  is Some(band) and x >= W - band.thickness -> band.bg_color
else                                                          -> frame pixel
```

Top / bottom bands take precedence over left / right bands at the corners.
This matches the visual convention that sticky horizontal bars (header,
footer) typically span the full width on web pages, including over any
sticky sidebars.

A `mask` of `None` (detector not locked yet, or detection disabled) skips
the rewrite entirely and copies the slice as-is, preserving v0.2 behavior.

The first frame never flows through `append`; it is taken directly by
`accept_first_frame` and stored in `LinearCanvas::new`. The mask therefore
naturally does not affect the first frame, satisfying the
"preserve first frame" requirement.

## Stitcher Integration

`Stitcher` gains one field:

```rust
pub struct Stitcher {
    config: StitchConfig,
    canvas: Option<LinearCanvas>,
    last_good_frame: Option<RgbaImage>,
    last_good_signature: Option<Vec<u8>>,
    last_motion: (i32, i32),
    locked_axis: Option<ScrollAxis>,
    stats: StitchStats,
    static_detector: StaticRegionDetector,
}
```

`Stitcher::new` constructs the detector from `config.static_region`.

Inside `push_frame`, between successful verification and the call to
`canvas.append`, the detector is updated and queried:

```rust
// after PixelOverlapVerifier::Pass, before LinearCanvas::append:
if self.config.static_region.enabled {
    self.static_detector.observe(anchor, &frame, candidate.dx, candidate.dy);
}

let mask = if self.config.static_region.enabled {
    self.static_detector.mask()
} else {
    None
};

let added = canvas.append(direction, &frame, slice_px, mask)?;
```

`observe` is only called on the path that ends in a successful append. The
`Duplicate`, `NoProgress`, `NoMatch`, and `AxisChanged` paths do not call it:
they have no verified motion, and feeding noise into the detector would
destabilize the lock.

## Public API Re-exports

`rollshot-core` adds to its `lib.rs`:

```rust
mod static_region;
pub use static_region::{StaticMask, StickyBand, StaticRegionConfig};
```

`StaticRegionDetector` stays `pub(crate)`.

## Configuration

```rust
pub struct StitchConfig {
    /* existing fields preserved verbatim */
    pub static_region: StaticRegionConfig,
}

impl Default for StitchConfig {
    fn default() -> Self {
        Self {
            /* existing defaults preserved */
            static_region: StaticRegionConfig::default(),
        }
    }
}
```

The default for `StaticRegionConfig::enabled` is `true`. Tests that want to
compare against pre-v0.2.1 output can set it to `false`.

## Testing Strategy

### Test helpers

New helpers in `crates/rollshot-core/tests/common/mod.rs`:

```rust
/// Paint a fixed sidebar on the given side, full frame height.
pub fn paint_sticky_sidebar(frame: &mut RgbaImage, side: Side, width: u32);

/// Paint a fixed footer band along the bottom edge.
pub fn paint_sticky_footer(frame: &mut RgbaImage, height: u32);

/// Paint fixed top and / or bottom bands. Used for horizontal-scroll tests.
pub fn paint_sticky_horizontal_band(frame: &mut RgbaImage, top_h: u32, bottom_h: u32);
```

`paint_sticky_header` already exists from v0.2 and is reused.

### Unit tests

`crates/rollshot-core/src/static_region.rs` covers the detector in isolation:

```text
- pure_scroll_input_locks_with_all_none_bands
- left_sidebar_detected_with_locked_extent
- right_sidebar_detected_with_locked_extent
- top_header_detected_with_locked_extent
- bottom_footer_detected_with_locked_extent
- detector_returns_none_before_min_observations
- detector_locks_after_min_observations
- single_outlier_observation_does_not_shift_locked_extent
- extent_above_max_band_ratio_is_zeroed
- bg_color_is_channel_wise_median_of_inner_strip
- subsequent_observations_after_lock_are_noops
```

### Canvas unit tests

`crates/rollshot-core/src/canvas.rs` tests are mechanically updated to pass
`None` for the new `mask` parameter, and one new test is added per direction:

```text
- append_bottom_with_left_mask_fills_left_columns
- append_bottom_with_bottom_mask_fills_bottom_rows
- prepend_top_with_top_mask_fills_top_rows
- append_right_with_right_mask_fills_right_columns
- prepend_left_with_left_mask_fills_left_columns
- corner_overlap_lets_top_band_override_left_band
- none_mask_is_byte_identical_to_legacy_append
```

### Stitcher integration tests

New file `crates/rollshot-core/tests/static_region.rs`:

```text
- sticky_left_sidebar_does_not_duplicate_in_output
- sticky_right_sidebar_does_not_duplicate_in_output
- sticky_footer_does_not_duplicate_in_output
- sticky_header_motion_estimate_matches_v0_2_and_output_is_clean
- horizontal_scroll_with_sticky_top_band
- horizontal_scroll_with_sticky_left_band
- first_frame_keeps_sticky_pixels_verbatim
- no_sticky_baseline_output_byte_identical_to_v0_2
- detector_disabled_via_config_reproduces_v0_2_output
```

The `no_sticky_baseline_output_byte_identical_to_v0_2` test is the regression
gate. It captures a v0.2 golden image once at this version's baseline and
asserts byte-equality with the v0.2.1 default-config output on pure-scroll
fixtures. Any future tuning of detector thresholds that pulls a false positive
on a pure-scroll fixture will trip this test immediately.

### Performance

No new performance budget tests are added. The work added per frame is:

```text
detector.observe (only until lock, default 3 frames):
  row + column scans, each O((W + H) * overlap_dim)
  same order of magnitude as the existing PixelOverlapVerifier downsample pass

canvas.append mask application:
  O(slice_px * W) for vertical-axis append directions,
  O(slice_px * H) for horizontal-axis directions
  same order of magnitude as the existing copy_from in v0.2
```

After lock the per-frame cost is the mask application alone. The existing
`large_pair_stays_within_structural_search_budget` test in `matcher.rs` is
unaffected because the matcher path is unchanged.

## Acceptance Criteria

```text
[ ] static_region.rs module landed with the unit tests listed above
[ ] LinearCanvas::append signature updated; canvas unit tests pass with None
[ ] StaticRegionConfig added to StitchConfig with enabled=true by default
[ ] sticky_left_sidebar / sticky_right_sidebar / sticky_footer fixtures pass
[ ] sticky_header fixture: motion estimate unchanged from v0.2, output clean
[ ] horizontal_scroll_with_sticky_top_band passes
[ ] horizontal_scroll_with_sticky_left_band passes
[ ] first_frame_keeps_sticky_pixels_verbatim passes
[ ] no_sticky_baseline_output_byte_identical_to_v0_2 passes
[ ] detector_disabled_via_config_reproduces_v0_2_output passes
[ ] cargo test workspace passes
[ ] cargo fmt --check passes
[ ] cargo clippy --workspace --all-targets -- -D warnings passes
[ ] docs/rollshot_mvp_design.md is updated:
    - new section 3.2.1
    - section 13.4 deferral note updated
    - section 20 risk row updated to point at v0.2.1
```

## Risks

| Risk | Mitigation |
| --- | --- |
| Detector false-positives on pure-scroll fixtures | `motion_margin` guard, contiguous-from-edge requirement, `max_band_ratio` cap, byte-identical regression test. |
| Detector locks early on a transient overlay | Median over `min_observations` frames damps single outliers; users can lower `enabled` flag for diagnosis. |
| Sticky region has gradient / shadow / divider line | v0.2.1 accepts the visible seam. Carry-over from first frame is deferred to v0.2.2. |
| Self-animating sticky elements (spinner, video) | `motion_margin` test will reject them; `max_band_ratio` is the backstop. |
| Mid-frame floating UI (chat widget, banner) | Not anchored to an edge; contiguous-from-edge scan never reaches them. Explicitly out of scope. |
| API churn for downstream callers of `LinearCanvas::append` | `mask` is `Option<&StaticMask>`; callers passing `None` see no behavior change. |

## Future Work

- v0.2.2: Carry-over real sticky pixels from the first frame instead of
  flat-color fill.
- v0.5+: Semantic mask sources (OCR / DOM / accessibility tree) for cases
  where pixel-only detection cannot tell sticky content from scrollable
  content (e.g. fully solid uniform sidebars).
- Future: optional crop mode that removes sticky bands from the output and
  shrinks the canvas dimensions, behind a separate config flag.
