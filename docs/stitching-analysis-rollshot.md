# rollshot — Scrolling Screenshot Stitching Algorithm

## Summary

rollshot is a streaming, axis-locked scrolling-screenshot stitcher. Each incoming
RGBA frame is compared against the **last accepted frame** (the anchor, not the
entire canvas) using a coarse-to-fine pipeline — downsampled MAD coarse scan →
parallel SIMD NCC template refinement → 1-D edge-projection — with a **routine
per-frame FAST+KNN feature candidate** mixed into the same pool, all ranked by a
pixel-overlap verifier. On a locked axis the matcher first tries a cheap
**axis-locked fast path** (main-axis-only search plus a small cross-axis probe)
and only falls back to the full dual-axis search if that looks suspicious. Verified
motion produces a 1-D slice that is **pasted with overlap-and-overwrite** onto a
growing `StripCanvas` (a deque of pasted slices with a lazily-composed image cache
and periodic compaction) in one of four directions (Top / Bottom / Left / Right),
with the scroll axis locked after the first successful append.

The anchor's derived data (grayscale, coarse samples, edge projections, FAST
features) is cached on a `PreparedFrame` and reused across frames; only the current
frame is freshly prepared each round.

## Pipeline

```text
RgbaImage frame ──► Stitcher::push_frame()  (re-anchor bookkeeping)
                       │
                       └─► push_frame_inner()
                              │
                              ├─ first frame?          ──► StripCanvas::new(frame)  ──► FirstFrame
                              │                              last_good = PreparedFrame::new(frame)
                              │
                              ├─ anchor.dimensions() != frame.dimensions()? ──► NoMatch{DimensionMismatch}
                              │
                              ├─ duplicate::signature(frame)  (18×24 grayscale grid)
                              │   └─ vs. anchor sig?    ──► Duplicate
                              │
                              ├─ curr = PreparedFrame::from_parts(frame, signature)
                              │
                              ├─ matcher::estimate_motion(anchor, curr, locked_axis, last_motion)
                              │   │
                              │   ├─ if locked & axis_fast_path.enabled:
                              │   │     axis_fast_path_candidate (main-axis coarse+template+edge,
                              │   │       then cross_axis_check; accept unless suspicious)
                              │   │
                              │   ├─ coarse_candidates      (4× downsampled MAD, stride 8 in
                              │   │                           sample space; both V and H)
                              │   ├─ template_candidates    (parallel SIMD NCC refinement,
                              │   │                           seeded by predicted/coarse offset)
                              │   ├─ edge_projection_candidates (1-D MAD on row/col gradient sums)
                              │   ├─ feature_candidate_from_features (ROUTINE FAST+KNN candidate
                              │   │                           from cached anchor descriptors)
                              │   │
                              │   ├─ rank_verified_candidates
                              │   │   └─ PixelOverlapVerifier::verify
                              │   │       (legacy strict-mean OR confidence-gated tile-vote)
                              │   │
                              │   ├─ relaxed_coarse_candidate (retry with max_search_ratio = 0.85)
                              │   │
                              │   └─ feature_fallback_candidates (last-resort FAST+KNN brute-force)
                              │
                              ├─ accept_confidence gate (score > 0.15 → LowConfidence)
                              ├─ classify_direction (axis lock + axis_ratio_threshold)
                              ├─ locked_direction reversal? ──► NoMatch{ReverseDirection}
                              ├─ slice_px = |dx| or |dy|;  min_append check (< 8 → NoProgress)
                              ├─ PixelOverlapVerifier::verify (re-run, final gate)
                              │
                              └─ StripCanvas::append(direction, frame, slice_px)
                                     │
                                     └─ overlap-and-overwrite paste: push a cropped strip
                                        (widened to max(H/2, slice_px)) into the strip deque;
                                        compact_if_needed once strip bytes exceed
                                        COMPACT_FACTOR×logical ──► Appended
```

**Input**: a stream of fixed-dimension `image::RgbaImage` frames captured from
a user-selected screen region (the CLI feeds frames through `stitch_loop` in
`crates/rollshot-cli/src/cmd_capture.rs`; the app drives the `Stitcher` from
`crates/rollshot-app/src-tauri/src/session.rs`). The first accepted frame's
dimensions define `(W, H)` for the run; later frames whose dimensions differ
are rejected with `DimensionMismatch`.

**Output**: a single `RgbaImage` of size `(W, total_height)` for vertical
scrolls or `(total_width, H)` for horizontal scrolls, composed lazily by
`StripCanvas::image()` and exposed via `Stitcher::full_image()`.

The **default algorithm** is what `StitchConfig::default()` configures
(`crates/rollshot-core/src/types.rs:259-277`). The CLI uses the defaults and only
flips `fast_hnsw.enabled = false` when `--disable-feature-fallback` is passed
(`crates/rollshot-cli/src/cmd_capture.rs:126-129`). The app uses the defaults and
overrides only `min_overlap = 32` (`crates/rollshot-app/src-tauri/src/session.rs:222-223`).
The FAST+KNN feature matcher is **enabled by default**. There is no AKAZE backend.

## Algorithm: Coarse-to-Fine Template Matching with Routine Feature Matching

### Step 1 — Cheap reject: dimension and duplicate checks

`Stitcher::push_frame_inner` (`crates/rollshot-core/src/stitcher.rs:105`).

- If `anchor.dimensions() != frame.dimensions()` → `NoMatch{DimensionMismatch}`
  (`stitcher.rs:117-124`).
- `duplicate::signature(frame)` samples the frame on a **fixed 18 × 24 grid**
  (`SIGNATURE_COLS = 18`, `SIGNATURE_ROWS = 24` → 432 grayscale samples,
  `crates/rollshot-core/src/duplicate.rs:4-13`). `is_duplicate` takes the MAD
  against the anchor's signature, normalized to `[0, 1]`; if `<= 0.01`
  (`duplicate_threshold`) → `Duplicate` (`duplicate.rs:18-30`, `stitcher.rs:130-137`).

`signature` only *samples* the grid, so it is `O(432)` work per frame regardless
of frame size. Cheap.

### Step 2 — PreparedFrame: grayscale + cached derived data

`PreparedFrame` (`crates/rollshot-core/src/matcher.rs:1052-1135`).

A `PreparedFrame` owns the RGBA frame plus its matcher inputs. Grayscale is built
**eagerly** in `from_parts` via `to_grayscale` (`matcher.rs:1075-1092`, `:1137`)
as a `Vec<f32>` of luminance values (`0.299·R + 0.587·G + 0.114·B`), so the
downstream NCC and MAD passes never re-read RGBA. The coarse downsample
(`coarse()`), the two edge projections (`projection()`), and the FAST features
(`features()`) are each built **lazily on first use** behind a `OnceLock` and
cached (`matcher.rs:1111-1134`).

Crucially, the stitcher carries the accepted frame forward as
`last_good: Option<PreparedFrame>` (`stitcher.rs:18`, `:391`). The **anchor's**
grayscale, coarse samples, projections, and features are therefore computed once
and reused on every subsequent frame; only `curr` is freshly prepared each round
(`stitcher.rs:139-142`). On a steady scroll this halves the per-frame derived-data
work versus recomputing both sides.

### Step 3 — Coarse candidates (downsampled MAD)

`coarse_candidates` → `coarse_axis_candidate` (`crates/rollshot-core/src/matcher.rs:755-846`).

1. Downsample both grayscale buffers by `COARSE_DOWNSAMPLE_STEP = 4` using
   block-mean (`coarse_samples`, `matcher.rs:853`; cached on `PreparedFrame::coarse`).
   Sample dims = `(⌈W/4⌉, ⌈H/4⌉)` (`coarse_sample_dimensions`, `matcher.rs:848`).
2. For each enabled axis, enumerate offsets at stride `COARSE_AXIS_STRIDE = 8`
   in sample space (= 32 px in pixel space) via `coarse_axis_offsets`
   (`matcher.rs:1143`). For small sample dims (`min < 60`) the stride drops to 2.
   Range bounded by `max_search_ratio × dim / step` (default `0.4 × dim / 4`).
3. Score each offset with `coarse_mad` (`matcher.rs:877`): MAD over the overlap
   rectangle of the downsampled buffers, at step 1.
4. **Parallel** via `rayon::par_iter` over the offsets (`matcher.rs:824`). Pick
   lowest MAD; rescale `(dx, dy) *= 4` to pixel space (`matcher.rs:797-804`).
5. Filter by `candidate_matches_axis` (drops cross-axis-inconsistent offsets when
   locked).

In the **unlocked** / fallback path both V and H axes are searched
(`dual_search_axes`, `matcher.rs:515-519`). In the **axis-locked fast path** only
the main axis is scanned (see Step 8).

### Step 4 — Template candidates (parallel SIMD NCC refinement)

`template_candidates` → `search_template_axis` (`crates/rollshot-core/src/matcher.rs:556-673`).

1. **ROI selection** — `content_roi(W, H)` (`matcher.rs:734`) excludes
   `TOP_IGNORE_RATIO = 12%`, `BOTTOM_IGNORE_RATIO = 8%`, `SIDE_IGNORE_RATIO = 15%`
   (each min `MIN_IGNORE_PX = 24` px) from the frame edges to avoid app-chrome
   regions (status bars, taskbars).
2. **Match window** — `match_width_region(roi, match_width = 512)` centers a
   512-px-wide column band inside the ROI (`matcher.rs:745`). Load-bearing: NCC
   pixel work scales with the band area, not the full ROI area.
3. **Seed offset** — `template_seed` (`matcher.rs:541`) uses `last_motion`'s axis
   component when nonzero; otherwise falls back to the coarse candidate's offset.
   Steady-state scrolling thus scores only a small window around the predicted
   offset.
4. **Search window** — `refinement_offsets(seed, max_abs, radius)` (`matcher.rs:1171`).
   `template_refine_radius = COARSE_DOWNSAMPLE_STEP × COARSE_AXIS_STRIDE × 2 + 16
   = 80 px` (`matcher.rs:528`). At most `2·80 + 1 = 161` integer offsets per axis.
   `max_abs = min(dim − min_overlap, dim × max_search_ratio)`.
5. **NCC scoring** — `fast_ncc_score_shifted` (`matcher.rs:1321`) computes a
   normalized cross-correlation on the overlap of the match-width band shifted by
   `(dx, dy)`. It is a **single-pass, SIMD** kernel: `fused_sums_wide`
   (`matcher.rs:1244`) accumulates `Σx, Σx², Σy, Σy², Σxy` over each row using
   `wide::f32x8` lanes (`use wide::f32x8;` `matcher.rs:4`), folding each row's lane
   sums into `f64` before moving on (to keep the variance signal from collapsing on
   bright low-contrast pages). `ncc_from_sums` (`matcher.rs:1308`) then forms the
   Pearson correlation `num / sqrt(var_x · var_y)` from those summed-area moments,
   returning `f32::MIN` when either variance is `≤ 1.0`. The NCC score is mapped to
   a confidence as `1 − ncc.clamp(0, 1)` (lower is better, matching the MAD
   convention). A `#[cfg(test)]` `legacy_ncc_score_shifted` two-pass reference
   (`matcher.rs:1351`) exists only to assert the SIMD kernel matches it within
   `1e-4`.
6. Parallel via `rayon` over the offset list (`matcher.rs:633`).
7. Records the second-best score so `passes_second_best_margin` can reject
   periodic patterns (`second_best_margin = 0.001`, `matcher.rs:466`).

### Step 5 — Edge-projection candidates (1-D fallback signal)

`edge_projection_candidates` → `edge_projection_axis`
(`crates/rollshot-core/src/matcher.rs:911-981`).

1. `edge_projection(gray, axis)` (`matcher.rs:983`, cached on
   `PreparedFrame::proj_v`/`proj_h`) collapses the frame to a 1-D signal by summing
   `|∂gray/∂axis|` per row (vertical) or per column (horizontal), restricted to the
   ROI columns/rows for frames ≥ 1024 px on that dimension.
2. `projection_mad(prev_proj, curr_proj, offset, step = 2)` (`matcher.rs:1025`)
   finds the offset minimizing 1-D MAD between the projections, scanning the full
   range `[-max_offset, +max_offset]` (`signed_predict_iter`, `matcher.rs:1189`).
3. Cheap (`O(W + H)` per shift) but coarser than NCC; survives even when NCC on
   the band fails for low-texture content.

### Step 6 — Routine feature candidate (FAST + linear KNN)

Added to the candidate pool **every frame** when `fast_hnsw.enabled`
(`crates/rollshot-core/src/matcher.rs:239-252`):

1. `prev.features(...)` and `curr.features(...)` return cached `FrameFeatures`
   (FAST corners + 8-D descriptors). The anchor's features are cached on its
   `PreparedFrame`, so only `curr`'s are extracted this frame
   (`extract_frame_features`, `crates/rollshot-core/src/feature_matcher.rs:352`).
2. `feature_candidate_from_features` (`feature_matcher.rs:367`) runs a symmetric
   brute-force linear KNN (`BruteForceIndex` behind the `NearestDescriptors` trait,
   `feature_matcher.rs:313-343`) with the Lowe ratio test, then
   `vote_dominant_translation` (`feature_matcher.rs:216`) buckets the matches.
3. The resulting `MotionCandidate` competes in `rank_verified_candidates` alongside
   coarse/template/edge. It also carries `inliers`/`raw_matches`, which feed the
   verifier's confidence gate (Step 7 / Step 9).

This is in addition to the deeper last-resort feature fallback (Step 8). The
descriptor/KNN/vote details are shared with that path and described there.

### Step 7 — Verify & rank candidates

`rank_verified_candidates` (`crates/rollshot-core/src/matcher.rs:418-464`).

Each candidate is filtered by:
- `score > accept_confidence (0.15)` → drop.
- `passes_second_best_margin` → drop if periodicity detected.
- `candidate_matches_axis` → drop if axis-locked and cross-axis movement is
  inconsistent (`matcher.rs:473`).

Survivors go through `PixelOverlapVerifier::verify`
(`crates/rollshot-core/src/verifier.rs:26-102`). The verifier has two acceptance
paths:

- **Legacy strict-mean path.** Compute the downsampled MAD over the whole overlap
  at `downsample_step = 4`; reject if `> downsample_max_mad = 24/255 ≈ 0.094`.
  If that passes, lazily compute the full-resolution **sample-band** MAD on the
  trailing `sample_band = 160` rows (or cols) of the overlap; accept if
  `<= full_res_max_mad = 18/255 ≈ 0.071`. This path is byte-for-byte the old strict
  verifier — clean content takes it and produces identical output.
- **Robust tile-vote path** (reached only when the legacy mean fails). Run
  `tile_agreement` (`verifier.rs:154`): a full-overlap scan splitting the overlap
  into `robust_tile_px = 48`-px tiles and counting the fraction whose per-tile mean
  MAD is `<= robust_tile_tol = 10/255`. Accept iff that fraction `>=
  required_agreement(candidate)` (`verifier.rs:108`): a **strongly-supported**
  offset (NCC score `≤ 0.06`, or feature inlier-ratio `≥ 0.5`) drops to the
  misfire floor `robust_accept_ratio_floor = 0.6`; otherwise the strict
  `robust_accept_ratio = 0.85` applies. This tolerates a *localized* overlap
  change (e.g. a lazy-load image painting in), while the majority floor and the
  tight `tile_tol` still reject a global or uniform cross-axis-drift misfire.

The verifier is a **monotonic superset** of the old strict verifier. There is a
cost note in `verify` (`verifier.rs:71-78`): the tile scan is on the *reject* path
and runs once per candidate, so a reject-dominated frame pays a full-overlap scan
per candidate; clean steady-state frames return on the legacy path and never reach
it.

The accepted candidate's score is blended with `verifier_score × 0.5`
(`matcher.rs:449`). Lowest combined score wins.

### Step 8 — Relaxed coarse retry, then last-resort feature fallback

`relaxed_coarse_candidate` (`crates/rollshot-core/src/matcher.rs:375-416`).

If Steps 3–7 found nothing and `max_search_ratio < 0.85`, the coarse + template
passes are re-run with `max_search_ratio = 0.85` (`RELAXED_SEARCH_RATIO`,
`matcher.rs:373`) so a single fast scroll that jumped beyond `0.4·dim` can still be
recovered without paying for the feature fallback.

`feature_fallback_candidates` (`crates/rollshot-core/src/feature_matcher.rs:473-488`)
runs only when everything above failed. It dispatches to `fast_hnsw_candidates`
(`feature_matcher.rs:404`) when `fast_hnsw.enabled`, else returns `Disabled`.
There is no AKAZE branch.

> Despite the `FastHnsw` / `fast_hnsw_*` naming, **there is no HNSW in the code.**
> The matching is FAST corners + exact linear (brute-force) KNN. An HNSW (`hora`)
> backend was evaluated and reverted (≈43× slower at N ≤ 1200 plus a recall bug);
> the `NearestDescriptors` trait (`feature_matcher.rs:313`) is left as a seam, but
> the only impl is `BruteForceIndex`. The `Hnsw` identifiers are a historical
> reservation (see the module/`MatchMethod` doc comments,
> `feature_matcher.rs:1-6`, `types.rs:39-44`).

The FAST+KNN path (`fast_hnsw_candidates`):
1. `corners::corners_fast12` (falls back to `corners_fast9` if ≤ 200 corners),
   capped at `max_features = 1200` via stride subsampling (`extract_corners`,
   `feature_matcher.rs:21`).
2. `compute_descriptor` builds an **8-D row/col-mean descriptor** over a 9×9 patch
   (`descriptor_patch_size = 9`); corners too close to an edge are dropped
   (`feature_matcher.rs:80`).
3. `linear_knn_match` is a symmetric brute-force KNN with the Lowe ratio test
   (ratio 1.4) and `distance_threshold = 0.10`. Forward `curr → prev`, reverse
   `prev → curr`, keep mutual best pairs. Parallel via rayon
   (`feature_matcher.rs:144-196`).
4. `vote_dominant_translation` buckets each `(px − cx, py − cy)` translation into
   4-px bins; the largest bucket wins iff it is `≥ second_best_ratio = 2.0×` the
   runner-up and has `≥ min_inliers = 16` inliers, with at least
   `min_raw_matches = 24` raw matches and `min_keypoints = 80` per side. The median
   `(dx, dy)` in the bucket is returned, plus a median residual that scores the
   candidate (`feature_score`, `feature_matcher.rs:303`).
5. The candidate(s) are then fed back into `rank_verified_candidates` for the same
   pixel-overlap verification.

There is **no sub-pixel refinement** — all reported `(dx, dy)` are integer pixel
offsets. Coarse offsets are stride-32 quantized; template refinement lands them on
individual pixels.

### Step 9 — Axis-locked fast path

`axis_fast_path_candidate` (`crates/rollshot-core/src/matcher.rs:315-371`), run
first inside `estimate_motion` when a scroll axis is locked and
`axis_fast_path.enabled` (default true, `matcher.rs:205-213`).

- It runs coarse + template + edge candidates **on the main (locked) axis only**,
  ranks them with the same verifier, then runs `cross_axis_check`
  (`matcher.rs:675`): a `cross_axis_probe_radius = 6`-pixel NCC probe perpendicular
  to the lock.
- If the probe finds the best cross-axis offset beyond `max_cross_axis_px` or a
  meaningful residual improvement, the candidate is flagged `suspicious`; with
  `fallback_to_dual_axis_on_suspicious = true` (default) the fast path returns
  `None` and `estimate_motion` continues into the full dual-axis search.
- Config: `AxisFastPathConfig { enabled: true, cross_axis_probe_radius: 6,
  fallback_to_dual_axis_on_suspicious: true }` (`types.rs:223-239`).

The old "coarse always searches both V and H" statement is true only for the
unlocked path and the dual-axis fallback; on a locked axis the fast path is the
common case.

### Step 10 — Axis lock, slice size, final verify

Back in `push_frame_inner`:
- `accept_confidence` gate: `candidate.score > 0.15` → `NoMatch{LowConfidence}`
  (`stitcher.rs:179-190`).
- `classify_direction` (`stitcher.rs:482`) — first frame: `classify_axis` uses
  `axis_ratio_threshold = 1.5` to commit to V or H (else `Ambiguous`). Locked
  frames: `validate_with_lock` enforces `max_cross_axis_px = 6` and detects an
  axis change (`AxisChanged`) or `CrossAxisTooLarge`.
- `locked_direction` reversal: a direction opposite the latched one →
  `NoMatch{ReverseDirection}` (`stitcher.rs:236-250`).
- `slice_px = |dx|` or `|dy|`; if `< min_append (8)` → `NoProgress`
  (`stitcher.rs:252-266`).
- **Final verifier pass** — `PixelOverlapVerifier::verify` is re-run as the gate
  before paste (`stitcher.rs:268-302`), surfacing `InsufficientOverlap` or
  `OverlapVerificationFailed`. `min_overlap = 64` (default) / `32` (app).

### Step 11 — Composite onto the canvas

`StripCanvas::append` (`crates/rollshot-core/src/canvas.rs:164-226`).

`StripCanvas` (`canvas.rs:76-98`) holds a `VecDeque<CanvasStrip>` of pasted slices
(each an `RgbaImage` plus an `(x, y)` paste offset), a lazily-composed full-image
cache, and `logical_width`/`logical_height` tracking the virtual canvas size. The
v0.3 **overlap-and-overwrite** topology (`canvas.rs:1-34`):

- `overlap_px = max(0, frame_dim/2 − slice_px)`.
- `total_slice = min(slice_px + overlap_px, frame_dim)`.
- For `append_bottom` (`canvas.rs:228`): crop frame rows
  `[H − total_slice, H)` into a new strip, push it at `y = logical_height −
  overlap_px`, and grow `logical_height += slice_px`. The strip's overlap portion
  sits over the previous strip's tail; on compose, the later strip is drawn last
  and **overwrites** that overlap. `prepend_top`/`append_right`/`prepend_left` are
  symmetric (`canvas.rs:246-300`; prepend shifts existing strips by `slice_px`).
- `image()` (`canvas.rs:120`) composes lazily: `compose_if_needed` allocates one
  `logical_width × logical_height` buffer and `overlay_copy`s every strip into it,
  caching the result. The cache is invalidated on each append (`canvas.rs:223`).
- `compact_if_needed` (`canvas.rs:316`) collapses all strips into a single base
  strip once total strip bytes exceed `COMPACT_FACTOR = 2 ×` the logical-canvas
  byte size (`canvas.rs:61`), bounding resident memory while keeping append cost
  amortized `O(frame_h)`.
- `viewport()` (`canvas.rs:336`) crops a requested rect directly from the strips
  without composing the whole canvas.

This is a **direct paste** — no alpha blending, no seam carving. The
overlap-and-overwrite topology keeps only the most recent slice's pixels in the
overlap zone, so sticky/floating UI bars get **passively hidden** because each
frame's overlap rewrites the previous frame's content in that strip.

The old eager full-canvas-realloc `LinearCanvas` no longer exists in production;
a copy survives only as `LegacyLinearCanvas` inside `#[cfg(test)]`
(`canvas.rs:436`) to assert `StripCanvas` produces byte-identical output.

After the append, the stitcher latches `locked_axis`, `locked_direction`,
`last_motion`, and `last_good = curr` (`stitcher.rs:375-391`).

## Time Complexity

Let `W` = frame width, `H` = frame height, `S` = `max_search_ratio · max(W, H)`
(default `0.4 · max(W, H)`), `M` = `match_width` (default 512), `R` =
`template_refine_radius` (constant 80), `A` = overlap area (≤ `W·H`).

| Step | Cost | Notes |
| ---- | ---- | ----- |
| 1. duplicate signature + compare | `O(1)` per frame (432 samples) | Fixed 18×24 grid |
| 2. curr grayscale | `O(W·H)` | One pass; anchor gray is cached, not recomputed |
| 3. coarse downsample (curr) | `O(W·H)` | Block-mean; anchor coarse is cached |
| 3. coarse MAD scan (per axis) | `O((S/8) · A/16)` | Rayon-parallel over offsets. Sample-space area `≈ W·H/16`; stride-8 offsets in sample space → `≈ S/(4·8)` candidates per axis. Locked → one axis (fast path). |
| 4. template NCC refinement (per axis) | `O(R · M · H_band)` | Rayon-parallel over ≤ 161 offsets; each SIMD NCC visits ≤ `M × H_band` pixels once. Locked → one axis. |
| 5. edge projection | `O(W·H)` to build (cached), `O(S · (W+H))` to scan | 1-D MAD over full search range |
| 6. routine feature candidate | curr descriptors `O(N·16)` + KNN `O(N²)`, N ≤ 1200 | Every frame; anchor features cached |
| 7. verifier per surviving candidate | downsample MAD `O(A/16)`; sample-band MAD `O(min(160·W, 160·H))`; reject path adds a tile scan `O(A)` per candidate | Tile scan only on the reject path |
| 8. relaxed coarse retry | Same as steps 3+4 with `S' = 0.85·dim` | Only triggered on miss |
| 8. FAST+KNN fallback | corners `O(W·H)`; descriptors `O(N·16)`; KNN `O(N²)`; vote `O(N)` | Only on full miss |
| 9. axis fast-path cross probe | `O(13 · M · H_band)` | `2·radius+1 = 13` NCC scores |
| 10. axis/lock/append checks | `O(1)` | |
| 11. canvas append | amortized `O(frame_h · W)` (the cropped strip); a compaction frame spikes to `O(logical)` | Strip push between compactions; full recompose on compact |

**Steady-state per-frame cost** (no miss, locked vertical scroll, fast path):

```
T_steady = O(W·H)                          // curr grayscale + curr coarse downsample
         + O((S/32) · (W·H/16))            // coarse MAD scan, main axis only
         + O(R · M · H_band)               // template SIMD NCC, main axis (parallel over R)
         + O(13 · M · H_band)              // cross-axis probe
         + O(N² )                          // routine feature KNN, N ≤ 1200
         + O(verifier: A/16 + 160·W)       // verify the winner (legacy path)
         + O(frame_h · W)                  // canvas strip push (amortized)
```

The structural search-budget test (`crates/rollshot-core/src/matcher.rs:2124-2161`)
asserts `coarse_score_calls ≤ 4096`, `full_res_ncc_calls ≤ 768`, and
`full_res_ncc_pixel_visits ≤ 200M` on a 1470×900 pair, codifying the matcher
budget so regressions fail tests.

**Worst-case per-frame cost** (full miss → relaxed retry → FAST+KNN fallback):

```
T_miss = T_steady (dual-axis)
       + O(W·H)                            // FAST corners on curr (anchor cached)
       + O(N² )                            // symmetric linear KNN
       + O(A) per candidate                // verifier tile-vote on the reject path
```

**Asymptotically**, the matcher cost per frame is **independent of canvas size**
because it compares only `curr` against the anchor (both `W × H`). Unlike the old
eager canvas, the per-frame append no longer reallocates and copies the entire
stitched image: a normal append pushes only the cropped strip (amortized
`O(frame_h · W)`), and the full-canvas cost is paid only on the periodic
compaction frame (`O(logical)`) or when `image()`/`into_image()` composes the
output.

## Space Complexity

| Item | Size | Notes |
| ---- | ---- | ----- |
| anchor `PreparedFrame` (`last_good`) | `4·W·H` (rgba) + `4·W·H` (gray) + `4·W·H/16` (coarse) + `4·(W+H)` (proj) + features | Persists across frames; derived data cached |
| curr `PreparedFrame` (per call) | same shape as anchor | Dropped after the frame is processed (or promoted to anchor on append) |
| features (per side) | `≤ 1200 · ((u32,u32) coords + [f32;8] desc)` ≈ tens of KB | Anchor side cached; curr side built each frame |
| `StripCanvas` resident strips | bounded `≈ COMPACT_FACTOR × logical` bytes (`4 · W · canvas_h` × 2) | Compaction collapses to ~`1× logical` when the bound is hit |
| composed cache | `4 · W · canvas_h` when materialized | Built by `image()`/compaction; invalidated on append |

**Total resident**: `O(W · canvas_h)` — dominated by the canvas strips plus the
composed cache. Strip bytes are bounded to `≈ COMPACT_FACTOR (=2) × logical`; the
**peak RSS floor is `≈ (COMPACT_FACTOR + 1) × logical`**, because at the compaction
instant the old strips (up to `2×`) coexist with the freshly composed `1×` base
(`canvas.rs:42-61`). Peak transient during matching is roughly `4·W·H` (rgba)
+ `4·W·H` (curr gray) + `W·H/4` (curr coarse) per freshly-prepared frame, on top
of the cached anchor and the canvas.

## Edge Cases

- **No match** — `push_frame_inner` returns `StitchOutcome::NoMatch{reason,
  best_estimate}`. In the common case (≤ 1 miss, not stuck) the canvas, anchor
  frame, and lock are preserved and the next frame retries against the same anchor.
  **But after `REANCHOR_MISS_THRESHOLD = 2` consecutive content-disagreement
  misses** (`stitcher.rs:33`, `:51-95`) the stitcher re-anchors: if still on the
  first frame (`frame_count == 1`, nothing committed) `reanchor_to`
  (`stitcher.rs:450`) discards the stale anchor and rebuilds from the latest frame;
  mid-capture, `reanchor_mid_capture` (`stitcher.rs:462`) moves the match anchor to
  the latest frame but **preserves the committed canvas** (a content gap is logged).
  `ReverseDirection` is deliberately excluded from the miss count
  (`stitcher.rs:73-77`) so a valid scroll-back does not erode the direction guard.
  Reasons surfaced: `LowConfidence`, `AmbiguousAxis`, `CrossAxisTooLarge`,
  `InsufficientOverlap`, `OverlapVerificationFailed`, `NotEnoughFeatures`,
  `MotionTooSmall`, `DimensionMismatch`, `FeatureFallbackDisabled`,
  `FeatureLowInliers`, `ReverseDirection` (`crates/rollshot-core/src/types.rs:91-113`).
  There is **no** `AkazeLowInliers` variant.
- **Duplicate frames** — caught by the 18×24 grid signature before motion
  estimation (`stitcher.rs:126-137`).
- **Multi-axis (diagonal) scroll** — first frame: `classify_axis` requires the
  dominant axis to exceed `axis_ratio_threshold = 1.5×` the other; else
  `AmbiguousAxis`. Subsequent frames: `validate_with_lock` allows up to
  `max_cross_axis_px = 6` of cross-axis drift; beyond that, cross > main is reported
  as `AxisChanged` (handled but not appended in the same call), otherwise
  `CrossAxisTooLarge` (`crates/rollshot-core/src/axis.rs:54-99`). There is **no
  joint 2-D canvas** — `StripCanvas` grows along one locked axis.
- **Direction reversal** — `locked_direction` is latched on first append; a frame
  whose direction flips is rejected as `ReverseDirection` (`stitcher.rs:236-250`).
- **Fast scroll past `max_search_ratio`** — handled by the relaxed coarse retry
  (`max_search_ratio = 0.85`) before falling through to the feature fallback.
- **Periodic / repeating content** — `second_best_margin (0.001)` and the FAST
  bucket vote `second_best_ratio (2.0)` both protect against locking onto a
  periodic alias; the unit test `repeated_grid_is_rejected_by_second_best_margin`
  (`matcher.rs:2057`) exercises this.
- **Low-texture content** — edge-projection candidates and the FAST+KNN matcher
  (routine + fallback) exist for this case.
- **Localized overlap change (lazy-load image)** — accepted by the verifier's
  tile-vote path when the offset is strongly supported, while a global/uniform
  mismatch still fails the majority floor (`verifier.rs:79-101`).
- **Sticky UI bars (floating headers/footers)** — implicitly hidden by the
  overlap-and-overwrite canvas topology, without explicit detection.
- **Dimension change mid-stream** — rejected with `DimensionMismatch`; the canvas
  is preserved so the user can resize back.

## Optimizations

- **Single anchor instead of full canvas** — only the anchor `PreparedFrame` is
  matched; matcher cost is independent of how tall the canvas grows
  (`stitcher.rs:139-142`).
- **PreparedFrame derived-data cache** — the anchor's grayscale, coarse samples,
  edge projections, and FAST features are computed once and reused across frames;
  only `curr` is prepared each round (`matcher.rs:1052-1135`).
- **Coarse-to-fine** — 4× downsample + stride-8 sample-space scan keeps the
  full-range search cheap; template NCC refines on a ±80 px window around the
  predicted/coarse offset.
- **Axis-locked fast path** — once an axis is locked, the matcher searches the main
  axis only plus a 6-px cross-axis probe, falling back to full dual-axis search only
  when the probe looks suspicious (`matcher.rs:315-371`). The
  `locked_vertical_uses_main_axis_fast_path` test (`matcher.rs:2354`) asserts the
  fast path scores strictly fewer NCC calls than the dual-axis path.
- **`match_width` band** — only a 512-px-wide central column band is used for NCC,
  not the full ROI; load-bearing for keeping NCC cost bounded on wide retina-class
  frames (`types.rs:271`, `matcher.rs:745`).
- **Velocity-seeded template search** — `last_motion` seeds the refinement window,
  so steady scrolling pays only the ±80 px NCC cost (`matcher.rs:541`).
- **SIMD NCC kernel** — `fused_sums_wide` accumulates the NCC summed-area moments
  with `wide::f32x8` lanes, folding to `f64` per row for numerical stability
  (`matcher.rs:1244-1306`); the Pearson correlation is then formed in closed form
  by `ncc_from_sums` (`matcher.rs:1308`) — a single pass, not the old two-pass scan.
- **Rayon parallelism** — coarse offset scoring, template offset NCC, FAST
  descriptor computation, and KNN matching all use `par_iter`
  (`matcher.rs:633,824`; `feature_matcher.rs:129,173`).
- **Two-path verifier** — a cheap downsampled MAD gates the full-res sample-band
  MAD on the legacy path; the more expensive full-overlap tile-vote scan runs only
  on the reject path (`verifier.rs:48-101`).
- **Cheap duplicate short-circuit** — 432-sample signature beats the matcher for
  unchanged frames.
- **Border-aware ROI** — `content_roi` excludes top/bottom/side bands likely to be
  browser chrome / scrollbars (`matcher.rs:734`).
- **Strip canvas with compaction** — appends push a cropped strip
  (amortized `O(frame_h·W)`) instead of reallocating the whole canvas every frame;
  `compact_if_needed` bounds resident memory to `≈ 2× logical` and the lazy
  `image()` compose / `viewport()` crop avoid materializing the full canvas when not
  needed (`canvas.rs:42-61,164-360`).
- **Routine feature matching** — feeding a per-frame FAST+KNN candidate into the
  pool (not just as a last resort) supplies an inlier-ratio that lets the verifier
  accept strongly-supported offsets through the tile-vote path
  (`matcher.rs:239-252`, `verifier.rs:108-121`).
- **Overlap-and-overwrite paste** — passively hides sticky headers/footers without
  explicit detection, by always letting the newest slice overwrite the prior
  slice's tail in a `max(H/2, slice_px)`-tall paste band (`canvas.rs:228-300`).

There is **no GPU path** and **no multi-level image pyramid** beyond the single 4×
coarse downsample. Sub-pixel refinement is explicitly absent — all offsets are
integer pixels.

## Source References

- `crates/rollshot-core/src/lib.rs:12-19` — public surface (`Stitcher`,
  `StitchConfig`, `StripCanvas`, `VerifierConfig`, `FastHnswConfig`, …).
- `crates/rollshot-core/src/stitcher.rs:15-26` — `Stitcher` state
  (`canvas: Option<StripCanvas>`, `last_good: Option<PreparedFrame>`,
  `first_frame_misses`).
- `crates/rollshot-core/src/stitcher.rs:33` — `REANCHOR_MISS_THRESHOLD = 2`.
- `crates/rollshot-core/src/stitcher.rs:51-103` — `push_frame` (re-anchor
  bookkeeping wrapper).
- `crates/rollshot-core/src/stitcher.rs:105-404` — `push_frame_inner` (the
  top-level per-frame pipeline).
- `crates/rollshot-core/src/stitcher.rs:450-473` — `reanchor_to` /
  `reanchor_mid_capture`.
- `crates/rollshot-core/src/stitcher.rs:482-506` — `classify_direction`
  (axis-lock arbitration).
- `crates/rollshot-core/src/types.rs:259-277` — `StitchConfig::default()`.
- `crates/rollshot-core/src/types.rs:206-221` — `FastHnswConfig::default()`
  (FAST+KNN enabled by default).
- `crates/rollshot-core/src/types.rs:171-184` — `VerifierConfig::default()`
  (downsample/full-res MAD + robust tile-vote fields).
- `crates/rollshot-core/src/types.rs:223-239` — `AxisFastPathConfig::default()`.
- `crates/rollshot-core/src/types.rs:91-113` — `NoMatchReason` variants.
- `crates/rollshot-core/src/matcher.rs:172-313` — `estimate_motion` (orchestrator;
  fast path → coarse/template/edge → routine feature → rank → relaxed → fallback).
- `crates/rollshot-core/src/matcher.rs:315-371` — `axis_fast_path_candidate`.
- `crates/rollshot-core/src/matcher.rs:375-416` — `relaxed_coarse_candidate`.
- `crates/rollshot-core/src/matcher.rs:418-464` — `rank_verified_candidates`.
- `crates/rollshot-core/src/matcher.rs:556-673` — template SIMD NCC search.
- `crates/rollshot-core/src/matcher.rs:675-732` — `cross_axis_check`.
- `crates/rollshot-core/src/matcher.rs:755-846` — coarse downsampled MAD search.
- `crates/rollshot-core/src/matcher.rs:911-1023` — edge-projection 1-D matcher.
- `crates/rollshot-core/src/matcher.rs:1052-1135` — `PreparedFrame` (cached
  grayscale/coarse/projection/features).
- `crates/rollshot-core/src/matcher.rs:1244-1348` — `fused_sums_wide` /
  `ncc_from_sums` / `fast_ncc_score_shifted` (SIMD NCC).
- `crates/rollshot-core/src/verifier.rs:26-102` — `PixelOverlapVerifier::verify`
  (legacy strict-mean OR tile-vote).
- `crates/rollshot-core/src/verifier.rs:108-121` — `required_agreement`
  (confidence gate for the tile-vote floor).
- `crates/rollshot-core/src/verifier.rs:154-197` — `tile_agreement`.
- `crates/rollshot-core/src/feature_matcher.rs:313-343` — `NearestDescriptors`
  trait + `BruteForceIndex` (only impl).
- `crates/rollshot-core/src/feature_matcher.rs:352-360` — `extract_frame_features`.
- `crates/rollshot-core/src/feature_matcher.rs:367-402` —
  `feature_candidate_from_features` (routine per-frame candidate).
- `crates/rollshot-core/src/feature_matcher.rs:404-471` — `fast_hnsw_candidates`
  (FAST+KNN, brute-force; not HNSW).
- `crates/rollshot-core/src/feature_matcher.rs:473-488` —
  `feature_fallback_candidates` (last-resort dispatch).
- `crates/rollshot-core/src/feature_matcher.rs:216-277` —
  `vote_dominant_translation` (4-px bucket voting on KNN match deltas).
- `crates/rollshot-core/src/canvas.rs:1-61` — overlap-and-overwrite topology doc +
  `COMPACT_FACTOR`.
- `crates/rollshot-core/src/canvas.rs:76-98` — `StripCanvas` / `CanvasStrip`.
- `crates/rollshot-core/src/canvas.rs:120-128` — lazy `image()` / `into_image()`.
- `crates/rollshot-core/src/canvas.rs:164-300` — `append` + per-direction paste.
- `crates/rollshot-core/src/canvas.rs:302-334` — `compose_if_needed` /
  `compact_if_needed`.
- `crates/rollshot-core/src/canvas.rs:436` — `LegacyLinearCanvas` (test-only
  equivalence oracle; not the production canvas).
- `crates/rollshot-core/src/overlap.rs:10-47` — `compute_overlap` (rectangle
  geometry, shared by matcher and verifier).
- `crates/rollshot-core/src/axis.rs:23-99` — axis classification and lock
  validation.
- `crates/rollshot-core/src/duplicate.rs:4-50` — 18×24 grid signature for cheap
  duplicate detection.
- `crates/rollshot-cli/src/cmd_capture.rs:126-129` — CLI `StitchConfig` usage
  (`--disable-feature-fallback` flips `fast_hnsw.enabled = false`).
- `crates/rollshot-app/src-tauri/src/session.rs:222-223` — Tauri app
  `StitchConfig` usage (overrides only `min_overlap = 32`).
