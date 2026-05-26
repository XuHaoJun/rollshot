# rollshot — Scrolling Screenshot Stitching Algorithm

## Summary

rollshot is a streaming, axis-locked scrolling-screenshot stitcher. Each incoming
RGBA frame is compared against the **last accepted frame** (not the entire canvas)
using a three-tier coarse-to-fine pipeline — downsampled MAD coarse scan →
parallel NCC template refinement → optional FAST+KNN feature fallback — followed
by a two-stage pixel-overlap verifier. Verified motion produces a 1-D slice
that is **pasted with overlap-and-overwrite** onto a single growing `LinearCanvas`
in one of four directions (Top / Bottom / Left / Right), with the scroll axis
locked after the first successful append.

## Pipeline

```text
RgbaImage frame ──► Stitcher::push_frame()
                       │
                       ├─ first frame?           ──► LinearCanvas::new(frame)  ──► FirstFrame
                       │
                       ├─ duplicate::signature   (18×24 grayscale grid)
                       │   └─ vs. last sig?      ──► Duplicate
                       │
                       ├─ matcher::estimate_motion(prev=last_good_frame, curr=frame, locked_axis, last_motion)
                       │   │
                       │   ├─ to_grayscale(prev) / to_grayscale(curr)        ─► Vec<f32> at f32 luminance
                       │   │
                       │   ├─ coarse_candidates  (4× downsampled MAD,
                       │   │                       stride 8 in sample space = 32 px)
                       │   ├─ template_candidates (parallel NCC, refinement
                       │   │                       window seeded by predicted/coarse offset)
                       │   ├─ edge_projection_candidates (1-D MAD on row/col gradient sums)
                       │   │
                       │   ├─ rank_verified_candidates
                       │   │   └─ PixelOverlapVerifier::verify (downsampled MAD → sample-band MAD)
                       │   │
                       │   ├─ relaxed_coarse_candidate  (retry with max_search_ratio = 0.85)
                       │   │
                       │   └─ feature_fallback_candidates
                       │       ├─ AKAZE (opt-in, expensive, off by default)
                       │       └─ FAST corners + linear-KNN ratio-tested matching   ◄── default
                       │
                       ├─ classify_direction (axis lock + axis_ratio_threshold)
                       ├─ slice_px = |dx| or |dy|;  min_append check
                       ├─ PixelOverlapVerifier::verify (re-run, final gate)
                       │
                       └─ LinearCanvas::append(direction, frame, slice_px)
                              │
                              └─ overlap-and-overwrite paste (slice widens to
                                 max(H/2, slice_px), pastes back overlap_size
                                 pixels onto the existing tail) ──► Appended
```

**Input**: a stream of fixed-dimension `image::RgbaImage` frames captured from
a user-selected screen region (`crates/rollshot-app/.../session.rs` line 208,
`crates/rollshot-cli/src/cmd_capture.rs` line 256). The first accepted frame's
dimensions define `(W, H)` for the run; later frames whose dimensions differ
are rejected with `DimensionMismatch`.

**Output**: a single `RgbaImage` of size `(W, total_height)` for vertical
scrolls or `(total_width, H)` for horizontal scrolls, available via
`Stitcher::full_image()`.

The **default algorithm** is what `StitchConfig::default()` configures —
verified at `crates/rollshot-cli/src/cmd_capture.rs:126` and
`crates/rollshot-app/src-tauri/src/session.rs:188` (the app only overrides
`min_overlap = 32`). AKAZE is **disabled by default**; FAST+KNN is **enabled by
default**.

## Algorithm: Coarse-to-Fine Template Matching with Feature Fallback

### Step 1 — Cheap reject: dimension and duplicate checks

`Stitcher::push_frame` (`crates/rollshot-core/src/stitcher.rs:39`).

- If `prev.dimensions() != curr.dimensions()` → `NoMatch{DimensionMismatch}`.
- `duplicate::signature(curr)` samples the frame on a **fixed 18 × 24 grid**
  (432 grayscale samples). MAD against the previous accepted frame's signature,
  normalized to `[0, 1]`. If `<= 0.01` (`duplicate_threshold`) → `Duplicate`.

This is `O(W·H)` for the grayscale conversion of `curr`? No — `signature` only
*samples* on the 18×24 grid, so it's `O(432)` work per frame. Cheap.

### Step 2 — Grayscale projection of both frames

`matcher::to_grayscale` (`crates/rollshot-core/src/matcher.rs:849`).

Both `prev` and `curr` are converted to a `Vec<f32>` of luminance values
(`0.299·R + 0.587·G + 0.114·B`) so the downstream NCC and MAD passes never
re-read RGBA. Cost: `O(W·H)` flops, single-threaded.

### Step 3 — Coarse candidates (downsampled MAD)

`coarse_candidates` (`crates/rollshot-core/src/matcher.rs:573`).

1. Downsample both grayscale buffers by `COARSE_DOWNSAMPLE_STEP = 4` using
   block-mean (`coarse_samples`, line 662). Sample dims = `(⌈W/4⌉, ⌈H/4⌉)`.
2. For each enabled axis (always both V and H, even when locked — see
   `search_axes` line 400 — so cross-axis change can still be detected),
   enumerate offsets at stride `COARSE_AXIS_STRIDE = 8` in sample space
   (= 32 px in pixel space) via `coarse_axis_offsets` (line 855). Range is
   bounded by `max_search_ratio × frame_dim / step` (default `0.4 × dim / 4`).
3. Score each candidate offset with `coarse_mad` (line 686): MAD over the
   overlap rectangle of the downsampled buffers, at step 1.
4. **Parallel** via `rayon::par_iter` (line 633). Pick lowest MAD; rescale
   `(dx, dy) *= 4` to pixel space.
5. Filter out anything that doesn't pass `candidate_matches_axis`.

### Step 4 — Template candidates (parallel NCC refinement)

`template_candidates` → `search_template_axis` (`crates/rollshot-core/src/matcher.rs:444-550`).

1. **ROI selection** — `content_roi(W, H)` (line 552) excludes
   `TOP_IGNORE_RATIO = 12%`, `BOTTOM_IGNORE_RATIO = 8%`, `SIDE_IGNORE_RATIO =
   15%` (min 24 px) from the frame edges to avoid app-chrome regions (status
   bars, taskbars).
2. **Match window** — `match_width_region(roi, match_width = 512)` centers a
   512-px-wide column band inside the ROI (`crates/rollshot-core/src/matcher.rs:563`).
   This is the load-bearing optimization: NCC pixel work scales with the band
   area, not the full ROI area.
3. **Seed offset** — `template_seed` (line 429) uses `last_motion`'s axis
   component when nonzero; otherwise falls back to the coarse candidate's
   offset. This makes steady-state scrolling tiny: only a small window around
   the predicted offset is scored.
4. **Search window** — `refinement_offsets(seed, max_abs, radius)` (line 883).
   `template_refine_radius = COARSE_DOWNSAMPLE_STEP × COARSE_AXIS_STRIDE × 2 +
   16 = 80 px`. So at most `2·80 + 1 = 161` integer offsets per axis.
   `max_abs` is `min(dim − min_overlap, dim × max_search_ratio)`.
5. **NCC scoring** — `ncc_score_shifted` (line 916) computes normalized
   cross-correlation on the overlap of the match-width band shifted by
   `(dx, dy)`. Two-pass: pass 1 accumulates means, pass 2 accumulates
   correlation and variances. Returns `num / sqrt(var_p · var_c)`. Confidence
   reported as `1 − ncc.clamp(0, 1)` (lower is better, matches MAD convention).
6. Parallel via `rayon` over the offset list (line 506).
7. Records `second_best_score` so `passes_second_best_margin` can reject
   periodic patterns (line 351, `second_best_margin = 0.001`).

### Step 5 — Edge-projection candidates (1-D fallback signal)

`edge_projection_candidates` → `edge_projection_axis`
(`crates/rollshot-core/src/matcher.rs:720-823`).

1. `edge_projection(gray, axis)` collapses the ROI to a 1-D signal by summing
   `|∂gray/∂axis|` per row (vertical) or per column (horizontal).
2. `projection_mad(prev_proj, curr_proj, offset, step = 2)` finds the offset
   minimizing 1-D MAD between the projections, scanning full range
   `[-max_offset, +max_offset]` (`signed_predict_iter` line 901).
3. Cheap (`O(W + H)` per shift) but coarser than NCC; survives even when NCC
   on the band fails for low-texture content.

### Step 6 — Verify & rank candidates

`rank_verified_candidates` (`crates/rollshot-core/src/matcher.rs:303`).

Each candidate is filtered by:
- `score > accept_confidence (0.15)` → drop.
- `passes_second_best_margin` → drop if periodicity detected.
- `candidate_matches_axis` → drop if axis-locked and cross-axis movement is
  inconsistent.

Survivors go through `PixelOverlapVerifier::verify`
(`crates/rollshot-core/src/verifier.rs:26`):
- **Pass A — downsampled MAD** over the whole overlap rectangle at
  `step = 4`. Reject if `> 24/255 ≈ 0.094`.
- **Pass B — sample-band MAD** at full resolution on the trailing
  `sample_band = 160` rows (or cols) of the overlap. Reject if `> 18/255 ≈
  0.071`.

The candidate's score is blended with `verifier_score × 0.5`. Best (lowest
combined score) wins.

### Step 7 — Relaxed coarse retry

`relaxed_coarse_candidate` (`crates/rollshot-core/src/matcher.rs:251`).

If steps 3–6 found nothing and `max_search_ratio < 0.85`, the coarse +
template passes are re-run with `max_search_ratio = 0.85` so a single fast
scroll that jumped beyond 0.4·dim can still be recovered without paying for
the feature fallback.

### Step 8 — Feature fallback (FAST + linear KNN)

Only runs when steps 3–7 all failed. `feature_fallback_candidates`
(`crates/rollshot-core/src/feature_matcher.rs:435`).

Dispatch is **pick-one**: if `akaze.enabled` (off by default) run AKAZE; else
if `fast_hnsw.enabled` (on by default) run FAST+KNN; else return `Disabled`.

The default FAST+KNN path (`fast_hnsw_candidates`,
`crates/rollshot-core/src/feature_matcher.rs:361`):
1. `corners::corners_fast12` (falls back to `corners_fast9` if <200 corners),
   capped at `max_features = 1200` via stride subsampling.
2. `compute_descriptor` builds an **8-D row/col-mean descriptor** over a 9×9
   patch (`descriptor_patch_size = 9`). Cheap (16 pixel reads / descriptor).
3. `linear_knn_match` is a symmetric brute-force KNN with Lowe ratio test
   (ratio 1.4) and distance threshold 0.10. Forward `curr → prev`, reverse
   `prev → curr`, keep mutual best pairs. Parallel via rayon.
4. `vote_dominant_translation` buckets each `(px − cx, py − cy)` translation
   into 4-px bins; the largest bucket wins iff it's `≥ 2.0 ×` the second-best
   bucket count and has `≥ 16` inliers (`min_inliers`). Median `(dx, dy)`
   inside the bucket is returned.
5. Candidate is then fed back into `rank_verified_candidates` for the same
   pixel-overlap verification.

There is **no sub-pixel refinement** — all reported `(dx, dy)` are integer
pixel offsets. Coarse offsets are stride-32 quantized; template refinement
lands them on individual pixels.

### Step 9 — Axis lock, slice size, final verify

Back in `Stitcher::push_frame`:
- `classify_direction` (line 289) — first frame: use `axis_ratio_threshold =
  1.5` to commit to V or H. Locked frames: `validate_with_lock` enforces
  `max_cross_axis_px = 6` and detects axis change.
- `slice_px = |dx|` or `|dy|`; if `< min_append (default 8, app: 32)` →
  `NoProgress`.
- **Final verifier pass** — `PixelOverlapVerifier::verify` is re-run as the
  gate before paste (line 162-187). `min_overlap = 64` (CLI) / `32` (app).

### Step 10 — Composite onto the canvas

`LinearCanvas::append` (`crates/rollshot-core/src/canvas.rs:93`).

The v0.3 **overlap-and-overwrite** topology
(`crates/rollshot-core/src/canvas.rs:1-34`):
- `overlap_size = max(0, frame_dim/2 − slice_px)`.
- `total_slice = slice_px + overlap_size` (clamped to `frame_dim`).
- For `append_bottom`: take frame rows `[H − total_slice, H)`; paste at
  `canvas_y = canvas_h − overlap_size` in a freshly-allocated
  `RgbaImage(canvas_w, canvas_h + slice_px)`. The new slice's top
  `overlap_size` rows **overwrite** the canvas's trailing rows; the bottom
  `slice_px` rows are net-new.
- Symmetric logic for `Top`, `Right`, `Left`.

This is a **direct paste** — no alpha blending, no seam carving. The "overlap
and overwrite" only keeps the most recent slice's pixels in the overlap zone,
so sticky/floating UI bars get **passively hidden** because each frame's
overlap rewrites the previous frame's content in that strip.

After the append, the stitcher latches `locked_axis`, `locked_direction`,
`last_motion`, `last_good_signature`, and `last_good_frame = frame`.

## Time Complexity

Let `W` = frame width, `H` = frame height, `S` = `max_search_ratio · max(W, H)`
(default `0.4 · max(W, H)`, default ≈ 0.4·H for vertical scrolls), `M` =
`match_width` (default 512), `R` = `template_refine_radius` (constant 80),
`A` = overlap area (≤ `W·H`).

| Step | Cost | Notes |
| ---- | ---- | ----- |
| 1. duplicate signature + compare | `O(1)` per frame (432 samples) | Fixed 18×24 grid |
| 2. to_grayscale (both frames) | `O(W·H)` | Two passes, single-threaded |
| 3. coarse downsample | `O(W·H)` | Block-mean, single-threaded |
| 3. coarse MAD scan (per axis) | `O((S/8) · A/16)` | Rayon-parallel over offsets. Sample-space area = `W·H/16`; per-axis stride-8 offsets in sample space → `S/(4·8)` candidates per axis. |
| 4. template NCC refinement (per axis) | `O(R · M · H_band)` per axis | Rayon-parallel over ≤ 161 offsets; each NCC visits ≤ `M × H` pixels twice (2-pass mean/variance). Band area is the match-width column × ROI height. Typical: 512 × 0.8·H. |
| 5. edge projection | `O(W·H)` to build, `O(S · (W+H))` to scan | 1-D MAD over full search range. |
| 6. verifier per surviving candidate | downsampled MAD `O(A/16)` + sample-band MAD `O(min(160·W, 160·H))` | At most a handful of survivors |
| 7. relaxed coarse retry | Same as steps 3+4 with `S' = 0.85·dim` | Only triggered on miss |
| 8. FAST+KNN fallback | corners: `O(W·H)`; descriptors: `O(N · 16)`; KNN: `O(N²)` with N ≤ 1200; vote: `O(N)` | Only on full miss. `N² = 1.44M` ops per direction, rayon-parallel. |
| 9. axis/lock checks | `O(1)` | |
| 10. canvas append | `O(W · (H + slice_px))` | Allocates a fresh RgbaImage and copies the old canvas plus the slice |

**Steady-state per-frame cost** (no miss, no fallback, vertical scroll):

```
T_steady = O(W·H)                          // grayscale + coarse downsample
         + O((S/32) · (W·H/16))            // coarse MAD scan, 2 axes
         + O(R · M · H)                    // template NCC, 2 axes (parallelized over R)
         + O(verifier: A/16 + 160·W)       // verify the winner
         + O(W · canvas_h)                 // canvas append (grows linearly)
```

With defaults (`S = 0.4·H`, stride 32, `M = 512`, `R = 80`): the dominant
matcher cost per frame is `O(R · M · H) ≈ 80 · 512 · H ≈ 40·960·H` NCC pixel
visits per axis, parallelized over the 161 offsets. The structural budget
test (`crates/rollshot-core/src/matcher.rs:1384`) verifies
`full_res_ncc_pixel_visits ≤ 200M` on a 1470×900 frame.

**Worst-case per-frame cost** (full miss → FAST+KNN fallback):

```
T_miss = T_steady
       + O(W·H)                            // FAST corners on both frames
       + O(N · 16)                         // 8-D descriptors, N ≤ 1200
       + O(N²)                             // symmetric linear KNN
       + O(W · canvas_h)                   // append (only if a fallback candidate verifies)
```

**Asymptotically dominant term** at typical frame sizes (1920×1080) is the
canvas append `O(W · canvas_h)` once `canvas_h ≫ H`, since it reallocates and
copies the entire stitched image each call. The matcher cost per frame is
**independent of canvas size** because it compares only `curr` vs.
`last_good_frame` (both of size `W × H`).

## Space Complexity

| Item | Size | Notes |
| ---- | ---- | ----- |
| `last_good_frame` | `4 · W · H` bytes | Anchor for next match (RGBA) |
| `last_good_signature` | `432` bytes | 18×24 grayscale sample grid |
| `prev_gray` / `curr_gray` | `2 · 4 · W · H` bytes | Per-call f32 luminance buffers (freed after `estimate_motion` returns) |
| coarse `prev_samples` / `curr_samples` | `2 · 4 · (W·H/16)` bytes | f32 downsampled buffers |
| edge projection vectors | `4 · (W + H)` bytes | Negligible |
| FAST corners + descriptors (fallback only) | `≤ 1200 · (8 bytes coord + 32 bytes desc) ≈ 48 KB` per frame | Only allocated on miss |
| `LinearCanvas.image` | `4 · W · canvas_h` (vertical) or `4 · canvas_h · H` (horizontal) | Grows with every accepted frame |
| Append scratch | `4 · W · (canvas_h + slice_px)` bytes | Freshly allocated then swapped in (see `crates/rollshot-core/src/canvas.rs:175-178`) |

**Total resident**: `O(W · canvas_h)` — dominated by the canvas. Peak transient
during an append is `≈ 2·` that (old canvas + new canvas live simultaneously
for the `copy_from` calls). Peak transient during matching is roughly
`4·W·H` (RGBA frame) + `8·W·H` (two f32 grayscale buffers) + `W·H/2` (coarse
samples) ≈ `12.5·W·H` bytes plus the canvas.

## Edge Cases

- **No match** — `Stitcher::push_frame` returns `StitchOutcome::NoMatch{reason,
  best_estimate}`. The canvas, anchor frame, and lock are **all preserved**, so
  the next frame retries against the same `last_good_frame`. Reasons surfaced:
  `LowConfidence`, `AmbiguousAxis`, `CrossAxisTooLarge`, `InsufficientOverlap`,
  `OverlapVerificationFailed`, `NotEnoughFeatures`, `FeatureLowInliers`,
  `AkazeLowInliers`, `FeatureFallbackDisabled`, `MotionTooSmall`,
  `DimensionMismatch`, `ReverseDirection` (`crates/rollshot-core/src/types.rs:93-116`).
- **Duplicate frames** — caught by the cheap 18×24 grid signature before
  motion estimation (`crates/rollshot-core/src/stitcher.rs:56-61`).
- **Multi-axis (diagonal) scroll** — first frame: `classify_axis` requires the
  dominant axis to exceed `axis_ratio_threshold = 1.5×` the other. If not,
  `AmbiguousAxis`. Subsequent frames: `validate_with_lock` allows up to
  `max_cross_axis_px = 6 px` of cross-axis drift; beyond that, if cross > main
  it's reported as `AxisChanged` (handled but never appended in the same
  call), otherwise `CrossAxisTooLarge`. There is **no joint 2-D canvas** —
  `LinearCanvas` only grows along one locked axis.
- **Direction reversal** — `locked_direction` is latched on first append. A
  frame whose direction flips (e.g. Bottom → Top on the same vertical axis)
  is rejected as `ReverseDirection` (`crates/rollshot-core/src/stitcher.rs:133-145`).
- **Fast scroll past `max_search_ratio`** — handled by the relaxed coarse
  retry (`max_search_ratio = 0.85`) before falling through to the feature
  fallback.
- **Periodic / repeating content** — `second_best_margin` (0.001) and the
  FAST bucket vote `second_best_ratio` (2.0) both protect against locking
  onto a periodic alias; the unit test
  `repeated_grid_is_rejected_by_second_best_margin`
  (`crates/rollshot-core/src/matcher.rs:1290`) exercises this.
- **Low-texture content** — edge-projection candidates and the FAST+KNN
  fallback exist for this case; AKAZE remains as an opt-in heavier fallback.
- **Sticky UI bars (floating headers/footers)** — implicitly hidden by the
  overlap-and-overwrite canvas topology (each new frame overwrites the
  previous frame's overlap zone), without explicit detection.
- **Dimension change mid-stream** — rejected with `DimensionMismatch`; the
  canvas is preserved so the user can resize back.

## Optimizations

- **Single anchor instead of full canvas** — only `last_good_frame` is
  matched; matcher cost is independent of how tall the canvas grows
  (`crates/rollshot-core/src/stitcher.rs:44-47`).
- **Coarse-to-fine** — downsample 4× + stride 8 sample-space scan keeps the
  full-range search cheap; template NCC refines on a ±80 px window around
  the predicted/coarse offset.
- **`match_width` band** — only a 512-px-wide central column band is used for
  NCC, not the full ROI; load-bearing for keeping `O(M · H)` NCC cost bounded
  on wide retina-class frames (`crates/rollshot-core/src/types.rs:267`,
  `crates/rollshot-core/src/matcher.rs:563`).
- **Velocity-seeded template search** — `last_motion` seeds the refinement
  window, so steady scrolling pays only the ±80 px NCC cost
  (`crates/rollshot-core/src/matcher.rs:429`).
- **Rayon parallelism** — coarse offset scoring, template offset NCC, FAST
  descriptor computation, and KNN matching all use `par_iter`
  (`crates/rollshot-core/src/matcher.rs:506,634`,
  `crates/rollshot-core/src/feature_matcher.rs:175,220`).
- **Two-stage verifier** — cheap downsampled MAD over the full overlap (4×
  step) gates the more expensive full-resolution 160-row sample-band MAD
  (`crates/rollshot-core/src/verifier.rs:48-62`).
- **Cheap duplicate short-circuit** — 432-sample signature beats the matcher
  for unchanged frames.
- **Border-aware ROI** — `content_roi` excludes top/bottom/side bands likely
  to be browser chrome / scrollbars (`crates/rollshot-core/src/matcher.rs:552`).
- **Pick-one feature fallback** — only one of AKAZE / FAST+KNN runs per miss;
  AKAZE off by default because it costs ~2 s on a 2560-wide frame
  (`crates/rollshot-core/src/types.rs:186-198`).
- **Structural search budget** — a unit test asserts `coarse_score_calls ≤
  4096`, `full_res_ncc_calls ≤ 768`, `full_res_ncc_pixel_visits ≤ 200M` on a
  1470×900 pair, codifying the optimization budget so regressions fail tests
  (`crates/rollshot-core/src/matcher.rs:1384-1421`).
- **Overlap-and-overwrite paste** — passively hides sticky headers/footers
  without explicit detection, by always letting the newest slice overwrite
  the prior slice's tail in a `max(H/2, slice_px)`-tall paste band
  (`crates/rollshot-core/src/canvas.rs:163-180`).

There is **no SIMD intrinsic code**, **no GPU path**, and **no image
pyramid** beyond the single 4× coarse downsample. Sub-pixel refinement is
explicitly absent — all offsets are integer pixels.

## Source References

- `crates/rollshot-core/src/lib.rs:1-19` — public surface (`Stitcher`,
  `StitchConfig`, `LinearCanvas`).
- `crates/rollshot-core/src/stitcher.rs:14-23` — `Stitcher` state.
- `crates/rollshot-core/src/stitcher.rs:39-264` — `push_frame` (the top-level
  per-frame pipeline).
- `crates/rollshot-core/src/stitcher.rs:289-313` — `classify_direction`
  (axis-lock arbitration).
- `crates/rollshot-core/src/types.rs:255-273` — `StitchConfig::default()`
  (the defaults this analysis describes).
- `crates/rollshot-core/src/types.rs:220-235` — `FastHnswConfig::default()`
  (FAST+KNN enabled by default).
- `crates/rollshot-core/src/types.rs:184-198` — `AkazeConfig::default()`
  (AKAZE disabled by default).
- `crates/rollshot-core/src/matcher.rs:127-247` — `estimate_motion` (the
  three-tier search orchestrator).
- `crates/rollshot-core/src/matcher.rs:251-301` — `relaxed_coarse_candidate`.
- `crates/rollshot-core/src/matcher.rs:303-349` — `rank_verified_candidates`.
- `crates/rollshot-core/src/matcher.rs:444-550` — template NCC search.
- `crates/rollshot-core/src/matcher.rs:573-655` — coarse downsampled MAD search.
- `crates/rollshot-core/src/matcher.rs:720-823` — edge-projection 1-D matcher.
- `crates/rollshot-core/src/matcher.rs:916-984` — `ncc_score_shifted` (two-pass
  normalized cross-correlation).
- `crates/rollshot-core/src/verifier.rs:13-70` — `PixelOverlapVerifier::verify`
  (downsampled MAD → sample-band MAD).
- `crates/rollshot-core/src/feature_matcher.rs:361-428` — `fast_hnsw_candidates`
  (default FAST+KNN feature fallback).
- `crates/rollshot-core/src/feature_matcher.rs:435-453` — `feature_fallback_candidates`
  (AKAZE / FAST+KNN pick-one dispatch).
- `crates/rollshot-core/src/feature_matcher.rs:263-324` — `vote_dominant_translation`
  (4-px bucket voting on KNN match deltas).
- `crates/rollshot-core/src/canvas.rs:1-34` — overlap-and-overwrite topology
  doc comment.
- `crates/rollshot-core/src/canvas.rs:93-161` — `LinearCanvas::append`
  (dispatch + axis lock).
- `crates/rollshot-core/src/canvas.rs:163-251` — per-direction paste
  implementations.
- `crates/rollshot-core/src/overlap.rs:10-47` — `compute_overlap` (rectangle
  geometry, shared by matcher and verifier).
- `crates/rollshot-core/src/axis.rs:23-99` — axis classification and lock
  validation.
- `crates/rollshot-core/src/duplicate.rs:1-50` — 18×24 grid signature for
  cheap duplicate detection.
- `crates/rollshot-cli/src/cmd_capture.rs:126-136` — CLI default `StitchConfig`
  usage (AKAZE only enabled via deprecated `--enable-akaze` flag).
- `crates/rollshot-app/src-tauri/src/session.rs:184-197` — Tauri app default
  `StitchConfig` usage (overrides only `min_overlap = 32`).
