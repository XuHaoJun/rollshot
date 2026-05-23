# Rollshot FAST + Linear-KNN Feature Fallback Design (v0.4)

Date: 2026-05-23

## Scope

This spec adds a FAST-corner / linear-KNN feature-matching fallback that
replaces AKAZE as the default last-resort matcher in
`estimate_motion`. AKAZE remains compiled (behind the existing `akaze`
feature flag), opt-in at runtime via `--enable-akaze`, and flagged as
deprecated; it will be removed in a future minor release.

The motivation: AKAZE on a 2560-wide frame stalls the capture loop for
~2 s per call because `rust-cv/akaze` is single-threaded multi-scale
feature extraction with no SIMD. Phase 1 (commit `a052a2a`) gated AKAZE
behind `--enable-akaze`. Phase 2 (commit `0f0be09`) added a relaxed
coarse pass that recovers most fast-scroll cases. Phase 3 (this spec)
gives `estimate_motion` a fast, always-on feature fallback for the
remaining cases without re-introducing the 2 s stall.

Matcher candidate pipeline, verifier, axis-lock, duplicate detection,
capture backends, canvas topology, and Stitcher state are untouched.

## Goals

- Replace AKAZE as the default feature-based fallback with a FAST + linear
  KNN matcher that runs in ~30–200 ms on a 2560-wide frame.
- Keep AKAZE compiled and runtime-switchable via `--enable-akaze` for
  parity validation and rollback. Emit a deprecation warning when used.
- Keep the public `Stitcher` API and `StitchOutcome` variants identical;
  add new `NoMatchReason` and `MatchMethod` variants only (the enums are
  already `#[non_exhaustive]`, so this does not break callers).
- Mirror the existing `AkazeConfig` + `AkazeCandidateOutcome` shape so
  the AKAZE removal in a future release is a delete-only change.
- Land with unit, integration, and CLI smoke tests at parity with the
  existing AKAZE test coverage.

## Non-Goals

- No HNSW approximate-nearest-neighbour search in this release. Matching
  is plain rayon-parallel linear KNN over `[f32; 8]` descriptors. (See
  "Alternative Approaches Considered" — Approach A — for when to revisit.)
- No binary BRIEF / ORB-style descriptors. Descriptor is a fixed
  `[f32; 8]` row/column-mean sketch of a 9×9 patch. (See Approach C.)
- No RANSAC geometric verification. Dominant-translation voting in a
  4 px bucket is the entire geometric check. RANSAC affine is a future
  upgrade if false-match rate becomes a problem.
- No prev-frame index caching in `Stitcher`. Each fallback invocation
  rebuilds corners + descriptors on the anchor frame. Fallbacks are rare
  and the build cost (~30–50 ms) is well below AKAZE's 2 s.
- No bench harness in this release. Validation is by unit + e2e tests on
  representative captures.
- No removal of AKAZE code in this release. Deprecation only.

## Architecture

```
estimate_motion (matcher.rs)
  │
  ├── coarse + template + edge candidates
  │     └── rank_verified_candidates → Candidate? → return
  │
  ├── relaxed_coarse_candidate (Phase 2)
  │     └── ranked candidate? → return
  │
  └── feature_fallback_candidates (NEW)
        │
        ├── if config.akaze.enabled   → akaze_candidates       (pick-one,
        │                                                       AKAZE wins)
        └── if config.fast_hnsw.enabled → fast_hnsw_candidates
        (both disabled → NoMatch { FeatureFallbackDisabled })
```

Pick-one dispatch: `--enable-akaze` is interpreted as an explicit opt-in
("I want AKAZE, not FAST"), so `fast_hnsw` is skipped when
`akaze.enabled` is true even if `fast_hnsw.enabled` is also true. This
mirrors `Phase 1`'s deprecation intent: AKAZE survives only as an
override path.

### File layout

| File | Change |
|---|---|
| `crates/rollshot-core/src/feature_matcher.rs` | NEW. Contains `fast_hnsw_candidates`, `feature_fallback_candidates`, internal helpers, `#[cfg(test)] mod tests`. |
| `crates/rollshot-core/src/akaze_matcher.rs` | Unchanged. |
| `crates/rollshot-core/src/matcher.rs` | `estimate_motion` calls `feature_fallback_candidates` instead of `akaze_candidates`. |
| `crates/rollshot-core/src/types.rs` | Add `FastHnswConfig`, add `MatchMethod::FastHnsw`, add `NoMatchReason::FeatureFallbackDisabled`, `NoMatchReason::FeatureLowInliers`. Add `StitchConfig::fast_hnsw`. |
| `crates/rollshot-core/src/lib.rs` | Re-export `FastHnswConfig`. |
| `crates/rollshot-core/Cargo.toml` | Add `imageproc` as a required (non-optional) dependency. Do **NOT** add `hora`. |
| `crates/rollshot-cli/src/args.rs` | Add `--disable-feature-fallback`. Mark `--enable-akaze` doc as deprecated. |
| `crates/rollshot-cli/src/cmd_capture.rs` | Wire `disable_feature_fallback`; emit deprecation `eprintln!` when `enable_akaze` is set. |
| `crates/rollshot-cli/src/cmd_stitch_folder.rs` | Same wiring + deprecation warning. |

`Stitcher` is unchanged. `LinearCanvas`, `verifier`, `axis`, `overlap`,
`duplicate` are unchanged.

## Modules and Types

### `FastHnswConfig` (`types.rs`)

```rust
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct FastHnswConfig {
    /// Enable the FAST + linear-KNN feature fallback. Default true.
    pub enabled: bool,
    /// FAST 9 / 12 luminance threshold. Default 64.
    pub corner_threshold: u8,
    /// Patch side for descriptor sampling (must be odd). Default 9 →
    /// `[f32; 8]` row/col descriptor.
    pub descriptor_patch_size: usize,
    /// Hard cap on corners kept after detection. Default 1200.
    pub max_features: usize,
    /// Per-side minimum keypoints to attempt matching. Default 80.
    pub min_keypoints: usize,
    /// Minimum raw matches after symmetric KNN. Default 24.
    pub min_raw_matches: usize,
    /// Minimum inliers inside the dominant translation bucket.
    /// Default 16.
    pub min_inliers: usize,
    /// Euclidean upper bound for a descriptor pair to be considered a
    /// match candidate. Default 0.10.
    pub distance_threshold: f32,
    /// Cross-axis tolerance in pixels (e.g. on a vertically-locked
    /// scroll, |dx| ≤ tolerance to accept). Default 2.
    pub cross_axis_tolerance: i32,
    /// Dominant bucket must have at least `second_best_ratio`× the
    /// votes of the runner-up. Default 2.0.
    pub second_best_ratio: f32,
}

impl Default for FastHnswConfig {
    fn default() -> Self { /* values above */ }
}
```

> **Naming note for implementers and reviewers.** The identifier
> `FastHnsw` is intentional and load-bearing: the API is named for the
> *planned* HNSW upgrade (Approach A) so a future swap from linear KNN
> to `hora::HNSWIndex` does not break the public type. **The current
> implementation is linear KNN.** Every type and function carrying the
> `FastHnsw` name MUST include a doc comment of the form:
>
> ```rust
> /// FAST corners + linear KNN matching. The "Hnsw" in the name is
> /// reserved for a future ANN upgrade — see
> /// docs/superpowers/specs/2026-05-23-rollshot-fast-hnsw-fallback-design.md
> /// Approach A. Current matching is exact linear scan.
> ```
>
> The comment is non-negotiable. PRs that add `FastHnsw*` identifiers
> without it are rejected.

### `FastHnswCandidateOutcome` (`feature_matcher.rs`)

```rust
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(/* mirrors AkazeCandidateOutcome */)]
pub(crate) enum FastHnswCandidateOutcome {
    Disabled,
    NotEnoughFeatures { prev: usize, curr: usize },
    NotEnoughMatches  { raw_matches: usize },
    Candidates(Vec<MotionCandidate>),
}
```

Shape is identical to `AkazeCandidateOutcome` so the `estimate_motion`
dispatch can be written once over an internal `FeatureFallbackOutcome`
union.

### `feature_fallback_candidates` (`feature_matcher.rs`)

```rust
pub(crate) enum FeatureFallbackOutcome {
    Disabled,
    NotEnoughFeatures { prev: usize, curr: usize },
    NotEnoughMatches  { raw_matches: usize },
    Candidates { candidates: Vec<MotionCandidate>, source: FeatureSource },
}

pub(crate) enum FeatureSource { FastHnsw, Akaze }

pub(crate) fn feature_fallback_candidates(
    prev: &RgbaImage,
    curr: &RgbaImage,
    locked_axis: Option<ScrollAxis>,
    config: &StitchConfig,
) -> FeatureFallbackOutcome;
```

Dispatch is the pick-one in §Architecture. `locked_axis` is forwarded
into `fast_hnsw_candidates` so cross-axis filtering is applied; the
AKAZE branch ignores `locked_axis` (no behavioural change from today).

### `fast_hnsw_candidates` (`feature_matcher.rs`)

```rust
pub(crate) fn fast_hnsw_candidates(
    prev: &RgbaImage,
    curr: &RgbaImage,
    locked_axis: Option<ScrollAxis>,
    config: &FastHnswConfig,
) -> FastHnswCandidateOutcome;
```

Private helpers (all module-private, no `pub`):

| Helper | Returns | Notes |
|---|---|---|
| `rgba_to_gray(img: &RgbaImage) -> GrayImage` | luma | Use `image::imageops::grayscale`. |
| `extract_corners(gray, threshold, max) -> Vec<(u32, u32)>` | corners | `corners_fast12` first; switch to `corners_fast9` if < 200; trim to `max`. |
| `compute_descriptor(gray, x, y, patch) -> [f32; 8]` | descriptor | Half-size = `patch/2` (=4 for default 9). 4 row-means + 4 col-means, normalized to `[0,1]`. |
| `compute_descriptors(gray, corners, patch) -> Vec<[f32; 8]>` | descriptors | rayon par_iter. |
| `linear_knn_match(prev_desc, curr_desc, config) -> Vec<[usize; 2]>` | `[curr_idx, prev_idx]` pairs | rayon par_iter over curr. For each curr, exact scan of prev for best/second Euclidean. Accept if `best.dist < distance_threshold` and `best.dist * 1.4 < second.dist` (Lowe ratio). |
| `vote_dominant_translation(prev_corners, curr_corners, matches, locked_axis, config) -> Option<(dx, dy, inliers, raw)>` | dominant translation | Bucket key = `(dx / 4, dy / 4)` with `dx, dy: i32` (integer division, no rounding). Cross-axis filter: on locked vertical reject `dx.abs() > config.cross_axis_tolerance`. Reject the `(0, 0)` bucket entirely. Inliers = matches whose `(dx, dy)` falls in winning bucket. Return bucket median `dx, dy`. |
| `feature_score(inlier_ratio, raw_matches) -> f32` | score in [0,1] | Reuse `akaze_matcher::akaze_score` if it can be made `pub(crate)`; otherwise duplicate the formula and add a `// keep in sync with akaze_score` comment. |

Descriptor type is hard-locked to `[f32; 8]`. Not `Vec<f32>`, not
generic. This is one of the spec's anti-leak invariants (§Alternative
Approaches Considered).

### Updates to existing types (`types.rs`)

```rust
pub enum MatchMethod {
    Template,
    Coarse,
    Edge,
    Akaze,
    FastHnsw,            // NEW
}

#[non_exhaustive]
pub enum NoMatchReason {
    // ... existing ...
    FeatureFallbackDisabled,  // NEW. Returned when both fast_hnsw and akaze are disabled.
    FeatureLowInliers,        // NEW. Returned from the FAST+KNN path.
    // AkazeDisabled, AkazeLowInliers — kept; the AKAZE branch still emits them.
}

pub struct StitchConfig {
    // ... existing ...
    pub fast_hnsw: FastHnswConfig,  // NEW. Default: enabled = true.
    pub akaze: AkazeConfig,          // Existing. Default: enabled = false (Phase 1).
}
```

## Data Flow

```
prev: RgbaImage              curr: RgbaImage
   │                            │
   ▼                            ▼
 rgba_to_gray              rgba_to_gray
   │                            │
   ▼                            ▼
 extract_corners          extract_corners
   │                            │
   │ Vec<(x,y)>                 │ Vec<(x,y)>
   │ (trimmed to max_features)
   ▼                            ▼
 compute_descriptors      compute_descriptors
   │                            │
   │ Vec<[f32; 8]>              │ Vec<[f32; 8]>
   │
   └──────────────┬─────────────┘
                  ▼
           linear_knn_match
                  │  Vec<[curr_idx, prev_idx]>
                  ▼
      vote_dominant_translation
                  │  (dx, dy, inliers, raw_matches)
                  ▼
        pass min_inliers /
        second_best_ratio?
                  │
       ┌──────────┴──────────┐
       │                     │
      yes                    no
       ▼                     ▼
  MotionCandidate       NotEnoughMatches /
  { dx, dy,             NotEnoughFeatures
    method: FastHnsw,
    score, inliers,
    raw_matches }
       │
       ▼
   Candidates(vec![candidate])
```

Key invariants:

- Descriptor computation skips corners whose patch reaches outside the
  image; no clamping, no panics. This is checked in
  `fast_hnsw_candidates_skips_edge_corners` (see Tests).
- `linear_knn_match` returns empty (not `None`) when either side is
  empty after corner extraction.
- `vote_dominant_translation` rejects the `(0, 0)` bucket so trivial
  no-motion cases cannot vote themselves into the winning bucket. (A
  zero-motion frame is filtered earlier by `Stitcher::is_duplicate`,
  but the matcher must still be defensive.)

## Error Handling and Outcome Mapping

Dispatch → outcome → `NoMatchReason`:

| `feature_fallback_candidates` outcome | `MotionSearchOutcome::NoMatch.reason` |
|---|---|
| `Disabled` | `FeatureFallbackDisabled` |
| `NotEnoughFeatures { .. }` (FAST or AKAZE) | `NotEnoughFeatures` |
| `NotEnoughMatches { .. }` from `FastHnsw` | `FeatureLowInliers` |
| `NotEnoughMatches { .. }` from `Akaze` | `AkazeLowInliers` |
| `Candidates { source: FastHnsw, .. }` rejected by `rank_verified_candidates` | `FeatureLowInliers` + `best_candidate = candidates.first()` |
| `Candidates { source: Akaze, .. }` rejected by `rank_verified_candidates` | `AkazeLowInliers` + `best_candidate = candidates.first()` |

`AkazeDisabled` is no longer emitted by `estimate_motion`; the dispatch
in `feature_fallback_candidates` collapses it into
`FeatureFallbackDisabled` when both sides are off. The variant stays in
the enum so external pattern-matches keep compiling; it is unused
internally.

`MotionSearchOutcome` itself is unchanged. The new `NoMatchReason`
variants surface to callers only through `StitchOutcome::NoMatch`,
which propagates them via the existing reporter / debug-report
serialization.

### Panic discipline

- Zero `.unwrap()` and zero `panic!()` in `feature_matcher.rs`.
- `linear_knn_match` and `vote_dominant_translation` use
  `slice.get(idx)` / `iter().min_by` style; no indexed access without
  bounds checks.
- Descriptor compute uses `f32` arithmetic; no `.sqrt()` on negatives;
  Euclidean is `(a - b).powi(2).sum::<f32>().sqrt()`.

## Test Strategy

### Layer 1 — `feature_matcher.rs` `#[cfg(test)]`

Mirrors `akaze_matcher::tests`.

| Test | Asserts |
|---|---|
| `fast_hnsw_candidates_estimate_translation` | Synthetic feature canvas + crop offset → `(dx, dy)` within ±3 px of ground truth, `method == FastHnsw`. |
| `fast_hnsw_candidates_returns_not_enough_features_on_solid_frames` | Solid-color frame → `NotEnoughFeatures`. |
| `fast_hnsw_candidates_returns_not_enough_matches_on_unrelated_frames` | Two unrelated feature canvases → `NotEnoughMatches`. |
| `fast_hnsw_candidates_respects_locked_vertical_axis` | `locked_axis = Some(Vertical)`, only-horizontal-offset input → `NotEnoughMatches`. |
| `fast_hnsw_candidates_respects_locked_horizontal_axis` | Symmetric to above. |
| `fast_hnsw_candidates_skips_edge_corners` | Corner adjacent to the image boundary does not panic; descriptor either omits or zeros it (test asserts a value, the spec accepts either as long as no panic). |
| `fast_hnsw_score_matches_akaze_default_accept_confidence` | Healthy inlier ratio → score below `StitchConfig::default().accept_confidence`. |

### Layer 2 — `crates/rollshot-core/tests/stitcher.rs`

| Test | Asserts |
|---|---|
| `fast_hnsw_fallback_recovers_repeated_grid_with_sparse_features` | The existing `make_akaze_fallback_canvas` fixture (repeated rows + sparse features): regular matchers fail, relaxed coarse fails, FAST+KNN recovers. Method is `FastHnsw`. |
| `fast_hnsw_attempt_with_blank_frames_reports_not_enough_features` | Two blank frames → `NoMatch { reason: NotEnoughFeatures }`. |
| `fast_hnsw_candidate_rejected_by_verifier_preserves_best_estimate` | FAST returns a candidate, verifier rejects → `NoMatch { reason: FeatureLowInliers, best_estimate: Some(_) }`. |
| `enable_akaze_overrides_fast_hnsw` | Build `StitchConfig` with both `akaze.enabled = true` and `fast_hnsw.enabled = true`. Use `make_akaze_fallback_canvas` from `tests/common/mod.rs`. Method must be `Akaze` (pick-one dispatch). |

### Layer 3 — CLI smoke (`crates/rollshot-cli/tests/cli_smoke.rs`)

| Test | Asserts |
|---|---|
| `rollshot_stitch_folder_default_uses_fast_hnsw` | On `make_akaze_fallback_smoke_canvas`, default run's debug report contains `"method": "FastHnsw"` and never `"Akaze"`. |
| `rollshot_stitch_folder_enable_akaze_overrides_fast_hnsw` (renames Phase 1's `_enable_akaze_toggle`) | With `--enable-akaze`, debug report contains `"method": "Akaze"` and never `"FastHnsw"`. |
| `rollshot_stitch_folder_enable_akaze_emits_deprecation_warning` | With `--enable-akaze`, stderr contains the substring `deprecated`. |
| `rollshot_stitch_folder_disable_feature_fallback_makes_low_inliers_no_match` | With `--disable-feature-fallback`, FAST is skipped and the fallback fixture returns `NoMatch { reason: FeatureFallbackDisabled }`. |

### Layer 4 — Golden fixtures

No new golden fixtures. The existing
`crates/rollshot-core/tests/golden_fixtures.rs` cases must all stay
green: they exercise the regular matcher path and must not start
diverting through the FAST fallback (a happy-path regression would
trip this).

## CLI / Configuration Changes

### `CaptureArgs` and `StitchFolderArgs` (`args.rs`)

```rust
/// Disable the FAST + linear-KNN feature fallback. The fallback only
/// runs after the regular matchers and the relaxed coarse pass both
/// miss; disabling is for benchmarking / debugging the matcher path.
#[arg(long, default_value_t = false)]
pub disable_feature_fallback: bool,

/// Enable the AKAZE feature-based fallback instead of FAST+KNN.
/// DEPRECATED — AKAZE will be removed in the next minor release.
/// Kept for parity testing during the FAST migration.
#[arg(long, default_value_t = false)]
pub enable_akaze: bool,
```

### Wiring (`cmd_capture.rs`, `cmd_stitch_folder.rs`)

```rust
let mut config = StitchConfig::default();
if args.disable_feature_fallback {
    config.fast_hnsw.enabled = false;
}
if args.enable_akaze {
    eprintln!(
        "warning: --enable-akaze is deprecated and will be removed in \
         a future release; FAST+KNN is the default feature fallback"
    );
    config.akaze.enabled = true;
}
```

`--enable-akaze` does **not** auto-disable `fast_hnsw`; the pick-one
dispatch in `feature_fallback_candidates` is the single source of
truth (akaze wins when both are enabled).

### `Cargo.toml` deltas

```toml
# crates/rollshot-core/Cargo.toml
[dependencies]
imageproc = { version = "<latest compatible with workspace image=0.25>", default-features = false }   # NEW, required
# NO hora entry. NO bitarray entry. Approach A and C are not in scope.
```

Pick the most recent `imageproc` release whose `image` peer matches the
workspace's `image = 0.25`. If that requires bumping `image`
workspace-wide, prefer the bump over pinning `imageproc` to an older
release.

## Migration / Deprecation

- AKAZE code paths (`akaze_matcher.rs`, `AkazeConfig`, `MatchMethod::Akaze`,
  `NoMatchReason::AkazeDisabled`, `NoMatchReason::AkazeLowInliers`, the
  `akaze` Cargo feature, the `--enable-akaze` CLI flag) all remain in
  this release.
- The `--enable-akaze` deprecation `eprintln!` is the user-facing
  signal. No deprecated `#[deprecated]` attribute on the public Rust
  types yet (would noise downstream callers; the deprecation lives in
  doc comments and CLI warning).
- Removal target: the next `v0.5` (or whichever minor follows v0.4
  shipping FAST+KNN). Removal is a separate PR with its own spec
  amendment.

## Alternative Approaches Considered (NOT to be implemented)

> ⚠️ **Current scope is Approach B only.** A and C are documented here
> so they don't get re-invented or partially leaked into the B
> implementation. If a PR mixes elements from A or C without an
> explicit amendment to this spec, reject it.

### Approach A — HNSW via `hora` crate (NOT this PR)

Replaces `linear_knn_match` with `hora::HNSWIndex<f32, usize>::search`.

- Dependency: `hora = "0.1"` (NOT to be added in this PR).
- Code-review red flags (reject if present without spec amendment):
  - `use hora::...` anywhere
  - `hora` entry in any `Cargo.toml`
  - `HNSWIndex`, `HNSWParams`, `Metric::Euclidean` identifiers
  - `ef_search`, `ef_build` fields on `FastHnswConfig`
- When to revisit: linear KNN measured > 200 ms on representative
  frames in production captures.

### Approach C — Binary BRIEF-style descriptor (NOT this PR)

Replaces the `[f32; 8]` row/col-mean descriptor with a binary
descriptor (e.g. `[u64; 2]` = 128-bit, Hamming distance) sampled from
the patch via fixed point-pair tests.

- Code-review red flags (reject if present without spec amendment):
  - `BitArray<N>` or `bitarray` crate import
  - `popcount`, `count_ones` used for descriptor matching
  - Random / table-driven point-pair sampling in `compute_descriptor`
  - Descriptor type other than `[f32; 8]`
- When to revisit: false-match rate (FAST+KNN returns a candidate that
  `rank_verified_candidates` rejects) > 10 % on a real capture session.

### `FastHnsw` naming under Approach B

The identifier is intentional: when (and only when) Approach A lands,
the type and function names need no churn. Until then, every
`FastHnsw*` identifier MUST carry the doc comment specified in
§Modules and Types. Reviewers must reject PRs that introduce
`FastHnsw*` identifiers without it.

## Open Questions

None. All design forks were resolved in the Phase 3 brainstorming
session preceding this spec.

## Out-of-Scope Notes

- Bench harness for the feature fallback path is deferred to a separate
  task (criterion-based, lives under `crates/rollshot-core/benches/`).
- Removing AKAZE entirely is a separate spec.
- Tuning `FastHnswConfig` defaults against real captures is deferred to
  post-merge.
