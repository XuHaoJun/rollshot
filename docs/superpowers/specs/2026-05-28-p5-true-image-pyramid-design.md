# P5 True Image Pyramid Design

## Goal

Add an always-on, production image pyramid candidate path to improve large
motion and 4K/retina frame recovery without replacing rollshot's existing
conservative safety model.

P5 is complete when pyramid candidates become the main large-motion recovery
mechanism. The implementation should remove `relaxed_coarse_candidate` if
benchmarks and golden fixtures show the pyramid covers the same recovery cases
without regressions. If one fixture still needs the legacy relaxed coarse path,
P5 may keep it temporarily, but the implementation must document the remaining
fixture and the condition for removal.

No public feature gate, Cargo feature, or `enabled` config is added. Once P5
lands, it is part of the default matcher behavior.

## Theory Check

The optimization is theoretically sound, with caveats that shape the design.

Image pyramids are a standard multiresolution representation: each level is a
lower-resolution version of the previous image. Gaussian pyramids blur before
subsampling, reducing aliasing before every 2x decimation. Coarse-to-fine
matching then searches a large displacement at the coarsest level, propagates
the offset by multiplying by 2 at each finer level, and only refines a small
residual at each level.

This is the same core idea used by pyramidal Lucas-Kanade tracking: each level
handles a small residual, while the accumulated motion can represent a much
larger full-resolution displacement. That property makes it a good fit for
rollshot's fast-scroll and 4K/retina cases.

The caveats are important:

- A pyramid does not prove the match is correct. It only proposes a candidate.
  `PixelOverlapVerifier` remains the final safety check.
- Repeated patterns can produce aliased or equally plausible offsets. In those
  cases `NoMatch` is acceptable; mis-append is not.
- Direct 2x2 box downsampling is simpler, but it is a weaker anti-aliasing
  filter. P5 uses a Gaussian-like separable `[1, 4, 6, 4, 1]` filter first.
- Pyramid scoring must obey rollshot's existing confidence contract: lower
  `MotionCandidate.score` is better, and `second_best_score` must remain useful
  for ambiguity rejection.

References checked while validating the theory:

- OpenCV Image Pyramids documentation:
  `https://docs.opencv.org/4.x/d4/d1f/tutorial_pyramids.html`
- Jean-Yves Bouguet, "Pyramidal Implementation of the Lucas Kanade Feature
  Tracker":
  `https://graphics.stanford.edu/courses/cs448a-00-fall/bouget00.pdf`

## Current Context

The relevant implementation is in `crates/rollshot-core/src/matcher.rs`.

- `estimate_motion` orchestrates dimension checks, optional axis fast path,
  coarse candidates, template candidates, edge projection candidates, relaxed
  coarse recovery, feature fallback, and verifier ranking.
- `PreparedFrame` already owns the eager grayscale buffer and lazy derived data
  for coarse samples and edge projections.
- `coarse_candidates` currently uses a single 4x downsampled representation.
  It is not a true multilevel pyramid.
- `template_candidates` prioritizes `last_motion` as the steady-scroll seed and
  falls back to coarse candidates when there is no useful history.
- `relaxed_coarse_candidate` retries a wider coarse search near the geometric
  ceiling after regular candidates fail. P5 targets this path for replacement.
- `rank_verified_candidates` already enforces confidence, second-best margin,
  axis compatibility, and `PixelOverlapVerifier`.
- Recent P3/P4 work added fused NCC and axis-fast-path behavior. P5 must keep
  those paths intact.

## Approved Decisions

- Use an always-on production pyramid path. No feature gate and no public
  `PyramidConfig.enabled`.
- Use a Gaussian-like separable `[1, 4, 6, 4, 1] / 16` blur before 2x
  decimation.
- Keep constants private to `matcher.rs` for the first implementation:
  `PYRAMID_MAX_LEVELS`, `PYRAMID_MIN_LEVEL_SIDE`, and
  `PYRAMID_REFINE_RADIUS`.
- Add pyramid as a candidate source, not as an append decision.
- Pyramid candidates must pass through existing `rank_verified_candidates`.
- Preserve `last_motion` as the preferred steady-scroll template seed.
- Use pyramid candidates as template seeds only when velocity history is absent
  or unusable for the jump being recovered.
- Treat `relaxed_coarse_candidate` as a replacement target. Remove it if P5
  validation covers its behavior; otherwise keep it as a documented temporary
  compatibility fallback.

## Rejected Alternatives

### Only Add Pyramid As A Parallel Candidate

This is low risk, but incomplete. It may add CPU and memory cost without
removing the large-motion recovery path it overlaps with.

### Immediately Delete Relaxed Coarse

This is architecturally cleaner, but too risky before validating repeated
pattern, low-texture, and 4K/retina fixtures. The design goal is replacement;
the implementation decision is evidence-driven.

### Use 2x2 Box Average Downsampling

Box averaging is deterministic and easy to implement, but P5's primary theory
risk is aliasing. Gaussian-like downsampling is the safer default for text,
thin lines, and repeated grids.

## Data Structures

Extend `PreparedFrame` with a lazy pyramid cache:

```rust
pub(crate) struct PreparedFrame {
    // existing fields...
    pyramid: OnceLock<FramePyramid>,
}
```

Add private matcher-local structures:

```rust
struct PyramidLevel {
    scale_log2: u8,
    width: u32,
    height: u32,
    gray: Vec<f32>,
}

struct FramePyramid {
    levels: Vec<PyramidLevel>,
}
```

Level 0 is the existing full-resolution grayscale buffer. To avoid duplicating
that buffer, the implementation may either:

- store level 0 by reference during search and only cache levels 1..N; or
- store a cloned level 0 if that keeps the code materially simpler.

The first implementation should prefer clarity, then verify memory impact in
benchmarks.

## Pyramid Construction

Pyramid construction is lazy. It is only built when the matcher reaches the
regular candidate path that needs it.

For each next level:

1. Horizontally blur the previous level with `[1, 4, 6, 4, 1] / 16`.
2. Vertically blur with the same kernel.
3. Decimate by taking every second pixel.
4. Use clamped edge sampling for deterministic borders.
5. Stop when `max_levels` is reached or either side would fall below
   `PYRAMID_MIN_LEVEL_SIDE`.

Dimension rules:

- New width is `prev_width.div_ceil(2)`.
- New height is `prev_height.div_ceil(2)`.
- The final level count includes level 0.

## Candidate Search

Add:

```rust
fn pyramid_candidates(
    prev: &PreparedFrame,
    curr: &PreparedFrame,
    locked_axis: Option<ScrollAxis>,
    config: &StitchConfig,
    metrics: &mut StitchMetrics,
) -> Vec<MotionCandidate>
```

Search behavior:

- Use the same `SearchAxis` abstraction as coarse/template/edge.
- If `locked_axis` exists, search only that main axis in the regular path where
  axis fast path has already failed or was not applicable.
- If no axis is locked, search both vertical and horizontal axes.
- At the coarsest level, run full-range axis search bounded by overlap geometry,
  not by the default `config.max_search_ratio`. This is what lets P5 cover the
  fast-scroll cases that currently need `relaxed_coarse_candidate`.
- For each finer level, multiply the offset by 2 and refine in
  `[-PYRAMID_REFINE_RADIUS, +PYRAMID_REFINE_RADIUS]`.
- Return at most one candidate per searched axis.

The search score can use MAD over the level grayscale buffers for the first
implementation. A future change may evaluate NCC at pyramid levels, but that is
not required for P5.

Candidate conversion:

- `MatchMethod` gains a `Pyramid` variant.
- Convert pyramid MAD to the existing confidence scale where lower is better.
- Preserve `second_best_score` on the same scale.
- Reject ambiguous candidates through the existing `second_best_margin`
  mechanism, not with a pyramid-specific rule.

## Matcher Integration

The regular `estimate_motion` path becomes:

```text
dimension check
axis fast path if locked_axis exists
regular path:
  coarse_candidates
  pyramid_candidates
  template_candidates
  edge_projection_candidates
  rank_verified_candidates

if no verified candidate:
  temporary legacy relaxed_coarse_candidate only if validation still needs it

feature fallback remains last
```

Important ordering rules:

- `pyramid_candidates` runs before `template_candidates` so its offset can be
  available as a template seed.
- `template_seed` keeps `last_motion` first when nonzero. This preserves the
  P4/P3 steady-scroll behavior where velocity history is the most reliable
  full-resolution seed.
- If `last_motion` is zero, missing, or clearly outside the available search
  range, `template_seed` may use the best coarse or pyramid candidate.
- Pyramid does not change duplicate handling, dimension mismatch handling,
  anchor updates, canvas topology, reverse direction policy, verifier
  thresholds, CLI behavior, or app behavior.

## Relaxed Coarse Replacement Rules

Before implementation work starts, capture the before benchmark JSONL:

```bash
rtk cargo bench -p rollshot-core --bench stitch_sequences -- \
    --out bench-results/runs/p5-pyramid/before.jsonl
```

Then implement P5 and capture:

```bash
rtk cargo bench -p rollshot-core --bench stitch_sequences -- \
    --out bench-results/runs/p5-pyramid/after.jsonl
rtk python3 scripts/bench/compare.py \
    bench-results/runs/p5-pyramid/before.jsonl \
    bench-results/runs/p5-pyramid/after.jsonl
```

Remove `relaxed_coarse_candidate` in P5 only if:

- the fast-scroll fixture currently covered by relaxed coarse still appends;
- large-motion 4K/retina fixtures do not regress;
- repeated rows/grid fixtures do not mis-append;
- low-feature text fixtures do not regress;
- golden fixture outcomes remain unchanged or improve only by reducing
  `NoMatch` without introducing wrong appends;
- structural search budgets remain bounded.

If relaxed coarse remains, the implementation must document the specific
fixture that still needs it and the condition for removing it later.

## Metrics

Use existing timing and counter fields where possible:

- `coarse_us`
- `template_ncc_us`
- `edge_projection_us`
- `verifier_us`
- `coarse_candidates`
- `ncc_offsets_scored`
- `ncc_pixel_visits`
- `verifier_candidates`

Add `pyramid_us` and `pyramid_candidates` to `StitchMetrics`. P5 introduces a
distinct matcher stage; hiding pyramid work inside `coarse_us` would make
before/after reports harder to interpret.

Do not add UI-facing metrics for P5.

## Testing Strategy

Add focused matcher unit tests in `crates/rollshot-core/src/matcher.rs`:

- `pyramid_downsample_dimensions_are_correct`
- `pyramid_gaussian_downsample_is_deterministic`
- `pyramid_large_jump_finds_correct_candidate`
- `pyramid_candidate_passes_existing_verifier`
- `pyramid_does_not_accept_repeated_grid_alias`
- `pyramid_score_contract_matches_ranker`
- `pyramid_can_replace_relaxed_coarse_on_fast_scroll_fixture`

Update or add structural budget tests:

- `large_pair_stays_within_structural_search_budget`
- a retina-scale ignored perf smoke if the existing one does not cover pyramid
  enough.

Golden and integration coverage must include:

- fast scroll beyond default `max_search_ratio`;
- 4K/retina pair;
- low-feature text;
- repeated rows;
- repeated grid;
- axis-locked steady vertical and horizontal scroll;
- axis change after lock.

## Acceptance Criteria

- No feature gate, no public config, no disabled-by-default path.
- `rtk cargo test -p rollshot-core` passes.
- `rtk cargo fmt --check` passes.
- `rtk cargo clippy -p rollshot-core --all-targets -- -D warnings` passes if
  the implementation touches shared matcher/config/metrics code.
- Before benchmark JSONL exists at
  `bench-results/runs/p5-pyramid/before.jsonl` before implementation changes.
- After benchmark JSONL exists at
  `bench-results/runs/p5-pyramid/after.jsonl`.
- Benchmark comparison exists for the before/after run.
- Fast-scroll large-motion `NoMatch` does not increase.
- Repeated rows/grid do not mis-append. `NoMatch` is acceptable.
- Golden fixture outcomes do not regress.
- If `relaxed_coarse_candidate` is removed, its previous recovery tests are
  covered by pyramid tests.
- If `relaxed_coarse_candidate` remains, the code or plan documents why and
  what evidence is required to remove it.
- Output remains visually unchanged except for intended recovery of previously
  missed large-motion frames.

## Non-Goals

- No phase correlation.
- No HNSW or feature fallback changes.
- No sub-pixel peak fitting.
- No verifier threshold changes.
- No canvas topology changes.
- No capture, CLI, or app changes.
- No public tuning API for pyramid constants in P5.
