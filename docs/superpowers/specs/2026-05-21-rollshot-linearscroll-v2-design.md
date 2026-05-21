# Rollshot LinearScroll v2 Design

Date: 2026-05-21

## Scope

This spec defines rollshot v0.2: a stronger platform-independent
`LinearScroll` stitching core.

v0.2 starts from the existing v0.1 core, which can stitch vertical downward
frame sequences using a dy-only template matcher. The new version replaces that
assumption with 2D motion estimates, automatic axis detection, four-direction
linear append, a single internal auto matcher strategy, and golden fixture
coverage.

The implementation is split into three dependent plans:

1. Foundation + Canvas
2. AutoHybrid Matcher
3. AKAZE + Fixtures + CI

## Goals

- Replace `OffsetEstimate { dy }` with `MotionEstimate { dx, dy, ... }`.
- Add `ScrollAxis` and `AppendDirection` as first-class core concepts.
- Automatically detect the first reliable scroll axis.
- Lock `LinearScroll` to one axis after detection.
- Support vertical and horizontal long screenshots.
- Support append directions `Bottom`, `Top`, `Right`, and `Left`.
- Keep matcher selection internal through one `AutoHybrid` strategy.
- Use cheap deterministic matchers before AKAZE.
- Add AKAZE as a required v0.2 fallback behavior for cases where template
  matching is ambiguous or weak.
- Gate AKAZE's external dependency behind a Cargo feature if that keeps the
  default build simpler, while making CI test the AKAZE path.
- Verify every accepted motion candidate with a generic 2D pixel overlap
  verifier.
- Add golden fixtures for core scroll directions, sticky headers, repeated
  content, low-feature frames, bad frames, duplicates, and AKAZE fallback.

## Non-Goals

- No new capture backend.
- No macOS overlay region selector.
- No interactive stop hotkey or floating control.
- No GUI or preview UI.
- No Mosaic2D canvas.
- No arbitrary 2D panning, diagonal stitching, loop closure, or global pose
  optimization.
- No Windows or Linux X11 support.
- No OpenCV ORB dependency.
- No general CLI option that lets users choose `template`, `fast`, or `akaze`.

## Architecture

`rollshot-core` remains the only owner of platform-independent stitching.
Capture crates still provide `RgbaImage` frames, and the CLI still consumes the
core API.

The v0.2 core flow is:

```text
RgbaImage frame stream
-> duplicate detection
-> AutoHybrid motion estimation
-> generic overlap verification
-> axis detection or axis-lock validation
-> four-direction LinearCanvas append
-> stitched RgbaImage output
```

`LinearScroll` remains a single-axis stitcher. It may grow in either direction
on that axis, but it does not become a 2D mosaic. If a reliable motion switches
from vertical to horizontal, or from horizontal to vertical, the stitcher rejects
that frame instead of changing modes.

## Public Core Types

The v0.1 public API exposes dy-only language:

```rust
pub struct OffsetEstimate {
    pub dy: i32,
    pub confidence: f32,
    pub method: MatchAlgorithm,
}
```

v0.2 replaces it with:

```rust
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MotionEstimate {
    pub dx: i32,
    pub dy: i32,
    pub axis: ScrollAxis,
    pub direction: AppendDirection,
    pub confidence: f32,
    pub method: MatchMethod,
    pub overlap: OverlapRegion,
    pub inliers: Option<usize>,
    pub raw_matches: Option<usize>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScrollAxis {
    Vertical,
    Horizontal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppendDirection {
    Bottom,
    Top,
    Right,
    Left,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatchMethod {
    Template,
    Coarse,
    Edge,
    Akaze,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OverlapRegion {
    pub prev_x: u32,
    pub prev_y: u32,
    pub curr_x: u32,
    pub curr_y: u32,
    pub width: u32,
    pub height: u32,
}
```

`dx` and `dy` describe the current frame's top-left position relative to the
previous accepted frame in content coordinates:

- `dy > 0` means the current frame sees lower content and appends `Bottom`.
- `dy < 0` means the current frame sees higher content and appends `Top`.
- `dx > 0` means the current frame sees content to the right and appends
  `Right`.
- `dx < 0` means the current frame sees content to the left and appends `Left`.

## Stitch Config

The regular CLI should not expose matcher choice. Core may keep internal tuning
knobs for tests, diagnostics, and debug commands.

```rust
#[derive(Debug, Clone, PartialEq)]
pub struct StitchConfig {
    pub strategy: MatchStrategy,
    pub min_overlap: u32,
    pub min_append: u32,
    pub duplicate_threshold: f32,
    pub accept_confidence: f32,
    pub axis_ratio_threshold: f32,
    pub max_cross_axis_px: i32,
    pub second_best_margin: f32,
    pub max_search_ratio: f32,
    pub verifier: VerifierConfig,
    pub akaze: AkazeConfig,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatchStrategy {
    AutoHybrid,
}
```

The default strategy is always `AutoHybrid`.

## Stitch Outcomes

`StitchOutcome` must explain why a frame did or did not change the output.

```rust
#[derive(Debug, Clone, PartialEq)]
pub enum StitchOutcome {
    FirstFrame,
    Appended {
        direction: AppendDirection,
        added: u32,
        estimate: MotionEstimate,
    },
    NoProgress {
        estimate: Option<MotionEstimate>,
    },
    Duplicate,
    NoMatch {
        reason: NoMatchReason,
        best_estimate: Option<MotionEstimate>,
    },
    AxisChanged {
        previous_axis: ScrollAxis,
        new_axis: ScrollAxis,
        estimate: MotionEstimate,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoMatchReason {
    LowConfidence,
    AmbiguousAxis,
    InsufficientOverlap,
    OverlapVerificationFailed,
    NotEnoughFeatures,
    MotionTooSmall,
    DimensionMismatch,
}
```

Bad frames must not poison the anchor. `NoMatch` and `AxisChanged` leave the
last accepted frame unchanged.

## Axis Detection And Locking

The first reliable non-duplicate motion estimate chooses the axis:

```text
abs(dx) > abs(dy) * axis_ratio_threshold -> Horizontal
abs(dy) > abs(dx) * axis_ratio_threshold -> Vertical
otherwise -> NoMatch(AmbiguousAxis)
```

The initial default `axis_ratio_threshold` should be `1.5`.

Once the axis is locked:

- Vertical mode accepts `Bottom` and `Top` only.
- Horizontal mode accepts `Right` and `Left` only.
- Cross-axis movement must stay within `max_cross_axis_px`.
- A reliable different-axis estimate returns `AxisChanged`.

`LinearScroll` never switches into Mosaic2D behavior.

## Linear Canvas

The append logic should move out of the current bottom-only helper into a
focused `LinearCanvas`.

```rust
pub struct LinearCanvas {
    image: RgbaImage,
    axis: Option<ScrollAxis>,
}
```

The canvas supports:

- `Bottom`: append the current frame's bottom non-overlap rows.
- `Top`: prepend the current frame's top non-overlap rows.
- `Right`: append the current frame's right non-overlap columns.
- `Left`: prepend the current frame's left non-overlap columns.

The canvas is still linear. In vertical mode its width stays constant. In
horizontal mode its height stays constant. Dimension mismatches return
`NoMatch(DimensionMismatch)`.

## Overlap Verification

The verifier must be generic over `dx` and `dy`, not special-cased to vertical
scroll.

For a candidate motion, compute the intersection between:

- the previous frame rectangle
- the current frame rectangle shifted by `(dx, dy)`

That intersection defines corresponding previous and current overlap regions.
The candidate is valid only when:

- overlap width and height are non-zero
- overlap area satisfies `min_overlap`
- downsampled grayscale MAD is below the configured threshold
- full-resolution ROI MAD is below the configured threshold

The verifier returns an `OverlapRegion` and a normalized confidence contribution
that the matcher uses for final ranking.

## AutoHybrid Matcher

`AutoHybrid` is the only production strategy. It may contain multiple internal
candidate generators, but callers receive one best `MotionEstimate`.

The pipeline is:

```text
1. DuplicateDetector
2. CoarseDownscaled2DMatcher
3. AxisAwareTemplateMatcher
4. EdgeOrColumnMatcher
5. AKAZE fallback
6. PixelOverlapVerifier
7. ConfidenceRanker
```

Candidate generators return:

```rust
pub struct MotionCandidate {
    pub dx: i32,
    pub dy: i32,
    pub method: MatchMethod,
    pub score: f32,
    pub second_best_score: Option<f32>,
    pub inliers: Option<usize>,
    pub raw_matches: Option<usize>,
}
```

The ranker rejects candidates with weak confidence, insufficient overlap, or a
too-small second-best margin. It then chooses the verified candidate with the
best final confidence.

## Template And Coarse Matching

The existing vertical template matcher should be evolved rather than replaced
all at once.

`CoarseDownscaled2DMatcher` searches a downscaled version of both frames to
produce rough `(dx, dy)` candidates. It uses `max_search_ratio` to avoid full
frame exhaustive search.

`AxisAwareTemplateMatcher` refines candidates along the relevant axis:

- Unknown axis: evaluate plausible vertical and horizontal candidates.
- Locked vertical axis: search primarily `dy`, with small cross-axis tolerance.
- Locked horizontal axis: search primarily `dx`, with small cross-axis
  tolerance.

Sticky headers, scrollbars, and portal crop borders should be handled first
through ROI exclusion. Semantic masks are out of scope for v0.2.

## AKAZE Fallback

AKAZE fallback is required behavior for v0.2, but the external dependency may
be Cargo-feature-gated.

The intended default is:

- `AutoHybrid` can call AKAZE when the binary is built with AKAZE support.
- CI must run tests for the AKAZE-enabled path.
- If the dependency is too heavy for default builds, use a feature named
  `akaze`.
- If the dependency proves lightweight and stable, making it a default
  dependency is acceptable.

AKAZE is not exposed as a normal user-facing algorithm choice.

AKAZE only estimates translation:

```text
prev point: (px, py)
curr point: (cx, cy)
candidate dx = px - cx
candidate dy = py - cy
```

The matcher buckets translation vectors, chooses the dominant bucket, computes
median inlier motion, applies axis-aware filtering, and then sends the result
through the same pixel verifier used by every other candidate.

Initial config:

```rust
#[derive(Debug, Clone, PartialEq)]
pub struct AkazeConfig {
    pub enabled: bool,
    pub max_features: usize,
    pub detector_threshold: f32,
    pub min_raw_matches: usize,
    pub min_inliers: usize,
    pub min_inlier_ratio: f32,
}
```

Defaults:

```text
enabled = true when AKAZE support is compiled
max_features = 1200
detector_threshold = 0.001
min_raw_matches = 24
min_inliers = 16
min_inlier_ratio = 0.35
```

## CLI And Debug Behavior

Normal commands should continue to look simple:

```bash
rollshot capture --output out.png
rollshot stitch-folder ./frames --output out.png
```

Do not add:

```bash
rollshot capture --algorithm akaze
rollshot capture --algorithm template
```

Debug-only controls are allowed:

```bash
rollshot stitch-folder ./frames --debug-match-report report.json
rollshot stitch-folder ./frames --dump-overlap-debug ./debug
rollshot stitch-folder ./frames --disable-akaze
```

`--disable-akaze` is a diagnostic switch, not a product-level algorithm picker.

## Fixtures And Tests

Core tests should include deterministic synthetic fixtures and golden image
fixtures.

Required fixture families:

- `linear_vertical_down`
- `linear_vertical_up`
- `linear_horizontal_right`
- `linear_horizontal_left`
- `sticky_header`
- `repeated_rows`
- `repeated_grid`
- `low_feature_text`
- `image_cards`
- `akaze_fallback`
- `bad_frame`
- `duplicate_frames`

Golden fixtures should include:

```text
frames/
  frame_000.png
  frame_001.png
  frame_002.png
expected/
  output.png
  motions.json
```

`motions.json` records expected per-frame motion:

```json
[
  { "frame": 1, "dx": 0, "dy": 180, "direction": "Bottom" },
  { "frame": 2, "dx": 0, "dy": 176, "direction": "Bottom" }
]
```

Test coverage must prove:

- duplicate frames do not append
- vertical down appends bottom
- vertical up prepends top
- horizontal right appends right
- horizontal left prepends left
- ambiguous axis is rejected
- axis changes are rejected
- bad frames do not poison the anchor
- sticky headers do not dominate motion
- repeated rows or grids do not append on ambiguous template matches
- AKAZE fallback recovers at least one fixture where template matching fails

## CI

The standard v0.2 verification set is:

```bash
rtk cargo fmt --check
rtk cargo clippy --workspace --all-targets -- -D warnings
rtk cargo test --workspace
rtk cargo test --workspace --features akaze
```

If AKAZE becomes a default dependency instead of a feature, replace the last
command with an equivalent targeted test command that proves AKAZE fallback ran.

Failed golden fixture tests should write artifacts under:

```text
target/test-artifacts/<fixture-name>/
  report.json
  overlap_prev.png
  overlap_curr.png
  diff.png
  matches.png
```

## Plan Split

### Plan 1: Foundation + Canvas

This plan changes the public core model and the append engine before matcher
complexity is introduced.

It covers:

- `MotionEstimate`
- `MotionCandidate`
- `ScrollAxis`
- `AppendDirection`
- `MatchMethod`
- `MatchStrategy::AutoHybrid`
- richer `StitchOutcome`
- `NoMatchReason`
- axis detection and locking
- `LinearCanvas`
- four-direction append
- generic overlap rectangle computation
- pixel verifier without AKAZE

The plan is complete when core tests can directly feed known motion estimates
into the canvas/verifier and prove all four append directions work.

### Plan 2: AutoHybrid Matcher

This plan evolves the existing dy-only matcher into the default auto strategy.

It covers:

- converting template matching to `MotionCandidate`
- vertical and horizontal template searches
- coarse downscaled 2D matching
- candidate verification with the generic overlap verifier
- candidate ranking
- second-best margin rejection
- sticky header and repeated-content behavior without AKAZE
- preserving the last good anchor on matcher failures

The plan is complete when vertical and horizontal synthetic scroll tests pass
without AKAZE.

### Plan 3: AKAZE + Fixtures + CI

This plan adds the expensive fallback path and makes the release quality bar
explicit.

It covers:

- AKAZE dependency decision: feature-gated or default dependency
- AKAZE keypoint extraction
- descriptor matching
- translation voting
- inlier filtering
- verifier integration
- debug match reports
- golden fixture layout
- AKAZE fallback fixture
- CI commands for baseline and AKAZE-enabled verification

The plan is complete when the golden fixtures pass and CI proves the AKAZE path
is maintained.

## Risks

| Risk | Mitigation |
| --- | --- |
| Axis detection chooses the wrong axis | Require axis ratio threshold, second-best margin, and verified overlap. |
| Real content changes axis mid-session | Return `AxisChanged`; keep Mosaic2D out of v0.2. |
| AKAZE slows every frame | Run cheap matchers first; call AKAZE only on weak or ambiguous candidates. |
| AKAZE dependency is costly | Gate dependency behind `akaze` while keeping CI coverage mandatory. |
| Repeated rows produce false matches | Use second-best margin, overlap verification, and AKAZE fallback. |
| Sticky headers dominate matching | Exclude top/bottom/side bands with ROI rules. |
| Debugging fixture failures is slow | Emit match reports and overlap/diff images for failing golden fixtures. |

## Success Criteria

v0.2 is done when:

- vertical down scroll produces a stable long screenshot
- vertical up scroll produces a stable long screenshot
- horizontal right scroll produces a stable wide screenshot
- horizontal left scroll produces a stable wide screenshot
- the core detects axis automatically
- the core rejects axis changes in `LinearScroll`
- the default CLI exposes no algorithm picker
- AKAZE fallback recovers a fixture that template matching cannot safely accept
- sticky header, repeated content, duplicate, bad-frame, and low-feature cases
  are covered by tests
- `cargo fmt`, `cargo clippy`, baseline tests, and AKAZE-enabled tests pass
