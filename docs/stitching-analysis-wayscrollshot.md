# wayscrollshot — Scrolling Screenshot Stitching Algorithm

## Summary

`wayscrollshot` is a Rust scrolling-screenshot tool for Wayland (wlroots
compositors) by `jswysnemc`. It captures successive screenshots of a user-
selected region via the external `grim` CLI, then stitches frames vertically
by aligning each new frame against the previous one. Four stitching modes
exist, but the **default** is `opencv-orb`: an OpenCV-backed ORB feature
matcher with Lowe's ratio test, an affine-partial-2D RANSAC fit, and a
template-matching fallback path for feature-poor frames.

Defaults are declared at
[`src/cli.rs:18-20`](../learn-projects/wayscrollshot/src/cli.rs)
(`#[default] OpenCvOrb`, enum value name `opencv-orb`).

## Pipeline

```
slurp ──► grim ──► RgbaImage ──► Stitcher.push_frame ──► canvas (RgbaImage)
                                       │
                                       └─► find_offset_opencv_orb
                                              │
                                              ├─ estimate_orb_offset (primary)
                                              ├─ find_offset_opencv_relaxed (loosen overlap)
                                              └─ find_offset_template_fallback (NCC)
```

1. **Region selection.** `slurp` is invoked (or a region is parsed from CLI/
   stdin) producing an `x,y wxh` rect.
   `src/capture.rs:10-67`.
2. **Frame capture.** A background thread shells out to `grim -g <region>
   -t png -l 0 -s 1 -` every `CAPTURE_INTERVAL` and decodes the PNG into an
   `image::RgbaImage`.
   `src/capture.rs:70-91`, `src/session.rs:215-241`.
3. **Duplicate suppression.** Before stitching, a cheap "signature" sample
   of a few rows/cols is compared to the previous one; identical frames are
   dropped so the stitcher only sees frames where the user has actually
   scrolled. `src/session.rs:226-239`.
4. **Stitcher dispatch.** `Stitcher::push_frame` either initializes the
   canvas (first frame) or computes a vertical offset for the new frame
   relative to `last_frame` and appends the new pixels.
   `src/stitch.rs:244-316`.
5. **Append.** A new canvas is allocated with height `full.height +
   new_height`. The previous canvas is copied in, then the bottom
   `new_height` rows of the incoming frame (rows `[overlap .. h)`) are
   pasted at `y = full.height`. No blending — pure paste.
   `src/stitch.rs:297-309`.
6. **Anchor frame.** For the ORB algorithm only, the **first** frame is
   kept as the alignment anchor; subsequent successful matches do NOT
   update `last_frame`. This is governed by
   `preserve_anchor = matches!(algorithm, Algorithm::OpenCvOrb)` at
   `src/stitch.rs:277` and the gated `update_last_frame` calls.
   Side effect: ORB only measures offsets relative to frame #1, so the
   region must stay scrolled-but-still-overlapping the first frame, or it
   falls through to relaxed/template paths.

## Algorithm: OpenCV ORB + BFMatcher + RANSAC affine

Implementation: `estimate_orb_offset` at
`src/stitch.rs:631-765`.

### Step 1 — Preconditions and grayscale conversion

- Reject if `prev.size != frame.size`, `w < 80`, or `h < 120`
  (`src/stitch.rs:638-643`).
- Convert both frames to 8-bit grayscale using the BT.601 luma formula
  (`0.299 R + 0.587 G + 0.114 B`) via `rgba_to_gray`
  (`src/stitch.rs:623-629`).
- Copy each `GrayImage` into an OpenCV `Mat` (CV_8UC1) pixel-by-pixel
  in `gray_to_mat` (`src/stitch.rs:767-779`).

### Step 2 — Feature mask

`build_feature_mask` (`src/stitch.rs:781-801`) builds a binary `Mat` that
restricts ORB to the **content ROI**, ignoring borders that are likely to
be UI chrome / scrollbars:

- Side margins: `max(0.04 * W, 24 px)` on left and right.
- Top margin: `max(0.12 * H, 24 px)`.
- Bottom margin: `max(0.08 * H, 24 px)`.

### Step 3 — ORB detect + describe

- `cv::ORB::create_def()` with `max_features = 1500`
  (`ORB_MAX_FEATURES`, `src/stitch.rs:31`).
- `orb.detect_and_compute_def` is run on both frames with the mask.
- Reject if either side has fewer than `ORB_MIN_KEYPOINTS = 80` keypoints
  or empty descriptors (`src/stitch.rs:667-673`).

### Step 4 — Brute-force kNN match + Lowe's ratio test

- `BFMatcher::create(NORM_HAMMING, false)` (cross-check off).
- `knn_train_match_def(curr_desc, prev_desc, &mut matches, 2)` returns 2
  nearest prev-descriptors per current descriptor.
- For each pair, accept only if `best.distance < 0.78 * second.distance`
  (Lowe's ratio, hardcoded 0.78 at `src/stitch.rs:691`).
- Additionally reject matches where `dy <= 1.0` (no upward motion or
  static) or `|dx| > 2 * ORB_MAX_DX = 24 px` (rejects horizontal drift).
- Need at least `ORB_MIN_MATCHES = 24` surviving matches
  (`src/stitch.rs:709-711`).

### Step 5 — RANSAC affine-partial-2D

`calib3d::estimate_affine_partial_2d(curr_points, prev_points, &mut
inliers, RANSAC, 3.0, 2000, 0.99, 10)`:

- 4-DOF model: rotation + uniform scale + translation.
- 3 px inlier threshold, 2000 max iterations, 0.99 confidence,
  refine-iters = 10.

### Step 6 — Validate the model

From the 2×3 affine `[a b tx; c d ty]`:

- `scale = (sqrt(a²+c²) + sqrt(b²+d²)) / 2`.
- `geom_drift = |a-1| + |d-1| + |b| + |c|` (a per-element distance from
  the identity rotation/scale block).
- Reject if any of these hold (`src/stitch.rs:739-746`):
  - `|tx| > ORB_MAX_DX (12 px)`
  - `ty <= 1.0` (no scroll)
  - `ty >= H - min_overlap` (offset would leave <120 px of overlap)
  - `|scale - 1| > 0.12` or `geom_drift > 0.12`
- Count inliers from the mask; need at least `ORB_MIN_INLIERS = 18`.

### Step 7 — Confidence score (lower is better)

```
inlier_ratio = inliers / raw_matches
confidence   = (1 - inlier_ratio) * 3.5
             + |tx| / ORB_MAX_DX
             + geom_drift * 6.0
```

Returned alongside `dy = ty`. The caller in `push_frame` rejects the
frame if `confidence > accept_diff (3.5)` (`src/stitch.rs:279-284`).
The final integer offset is `ty.round()`.

### Step 8 — Append (no blending)

`overlap = frame.height - offset`. The bottom `offset` rows of the new
frame are cropped (`imageops::crop_imm(&frame, 0, overlap, w, offset)`)
and pasted onto a freshly allocated canvas one row below the previous
canvas — straight `copy_from`, no alpha blending, no feathering, no seam.
`src/stitch.rs:297-309`.

### Fallback chain

If primary ORB returns `Ok(None)` or `Err`, two fallbacks fire in order
(`src/stitch.rs:520-533`):

1. **`find_offset_opencv_relaxed`** — rerun `estimate_orb_offset` with
   `min_overlap` shrunk by 40 px (floor 72). Adds `+0.45` confidence
   penalty. `src/stitch.rs:536-562`.
2. **`find_offset_template_fallback` → `find_offset_template_content`**
   — NCC template matching within the content ROI: pick a template of
   height `roi_h / 3` (>= 48 px) at the top of the ROI in the new frame,
   slide it over `prev` along a predicted-offset-first search order
   (`predict_offset_iter`). Accept only if best NCC ≥ 0.72 and beats
   second-best by ≥ 0.015, then verify with mean-abs-diff over up to
   160 overlap rows ≤ 18.0. `src/stitch.rs:820-904`.

## Time Complexity

Per frame, with `W = frame width`, `H = frame height`, `K_p`/`K_c` ORB
keypoint counts (capped at `ORB_MAX_FEATURES = 1500`), `M` raw matches
after ratio test (capped by `min(K_c, K_p)`), and ORB descriptors of
fixed length 32 bytes / 256 bits:

| Step                                          | Cost                       | Notes |
|-----------------------------------------------|----------------------------|-------|
| RGBA → grayscale (×2)                         | `O(W·H)`                   | Per-pixel BT.601 luma, two frames. `src/stitch.rs:623-629`. |
| GrayImage → cv::Mat copy (×2)                 | `O(W·H)`                   | Element-by-element `at_2d_mut` writes, not memcpy. `src/stitch.rs:767-779`. |
| Mask construction                             | `O(W·H)`                   | One `cv::rectangle` fill. `src/stitch.rs:781-801`. |
| ORB detect + describe (×2)                    | `O(W·H + K · 256)`         | OpenCV ORB: FAST detection over image pyramid plus 256-bit BRIEF per keypoint. Pyramid scan dominates ≈ `O(W·H)`. |
| BFMatcher kNN, k=2                            | `O(K_c · K_p · 256)`       | Brute-force Hamming; with K ≈ 1500 this is the dominant CPU cost in practice (~5.4 M descriptor-pair comparisons). |
| Ratio test + dx/dy filter                     | `O(K_c)`                   | Linear scan. `src/stitch.rs:683-707`. |
| `estimate_affine_partial_2d` RANSAC           | `O(I · M)`                 | `I = 2000` iterations × `M` matches for inlier counting; subset solves are constant-time (4-DOF, ≥2 pairs). `src/stitch.rs:714-723`. |
| Affine model validation + inlier count        | `O(M)`                     | `src/stitch.rs:729-757`. |
| Canvas allocation + copy + paste              | `O(W · H_canvas)`          | Full reallocation of the growing canvas every appended frame — see Space section. `src/stitch.rs:298-309`. |
| Fallback: template NCC (worst case)           | `O(W · roi_h · S)`         | `S ≤ H - template_h - skip_top`; per offset, ncc_score_region is `O(W · template_h)`. Early-exits on first score ≥ 0.95 or via the predicted-offset iterator. `src/stitch.rs:457-510`, `820-904`. |

**Total per frame (primary ORB path):**

```
O( W·H          // grayscale + mat copy + mask + ORB pyramid
 + K_c · K_p    // BFMatcher kNN (Hamming over 256-bit descriptors)
 + I · M        // RANSAC, I = 2000
 + W · H_canvas // append / canvas rebuild
)
```

With the configured caps `K ≤ 1500`, `I = 2000`, `M ≤ K`, the constant-
factor terms are bounded; for typical regions (W·H ≈ 300×1000) the
dominant cost is the kNN brute-force match plus the growing canvas
recopy.

If ORB falls through to the NCC fallback, add `O(W · roi_h · S)` where
`S` is the search range; `predict_offset_iter` orders by distance from
the predicted offset, so empirically it's far cheaper than worst case.

## Space Complexity

| Buffer                                        | Size                       |
|-----------------------------------------------|----------------------------|
| `full_image` canvas                           | `4 · W · H_canvas`         |
| `last_frame` anchor (RGBA)                    | `4 · W · H`                |
| Transient `combined` canvas during append     | `4 · W · (H_canvas + Δ)`   |
| Two grayscale buffers (`prev_gray`, `frame_gray`) | `2 · W · H`            |
| Two ORB `cv::Mat` clones                      | `2 · W · H`                |
| Mask `cv::Mat`                                | `W · H`                    |
| ORB keypoints + descriptors                   | `O(K · (sizeof(KeyPoint) + 32))` ≈ K · ~64 B |
| RANSAC inlier mask                            | `O(M)`                     |

The transient `combined` buffer at append time means the working set
**doubles** the canvas: peak RGBA memory is `~8 · W · H_canvas` plus a
roughly `~7 · W · H` working set for the matcher. For a tall capture
(e.g. 600 × 20000 px) the canvas alone is ~48 MB and peaks at ~96 MB
during append.

**Total:** `O(W · H_canvas + W · H + K)`.

## Edge Cases

- **No match.** `estimate_orb_offset` returns `Ok(None)` when keypoint
  counts, raw matches, inliers, or geometry checks fail. The caller
  tries the relaxed ORB pass, then the NCC template fallback. If
  everything fails, `find_offset_template` runs as a last resort
  (`src/stitch.rs:564-571`). When confidence still exceeds
  `accept_diff = 3.5`, `push_frame` returns `NoMatch` and — crucially
  for ORB — leaves the anchor frame in place rather than rotating to
  the bad frame (`src/stitch.rs:277-284`). Verified by the
  `opencv_orb_keeps_anchor_after_bad_frame` test
  (`src/stitch.rs:1367-1393`).
- **Insufficient overlap.** Rejected via `ty >= H - min_overlap`. The
  relaxed pass retries with `min_overlap` reduced by 40 px (floor
  72 px) and adds 0.45 to confidence
  (`src/stitch.rs:541-562`). Test `opencv_orb_relaxed_overlap_handles_
  large_jump` covers a 208 px jump on a 320 px tall frame
  (`src/stitch.rs:1421-1445`).
- **Tiny progress.** If `offset < min_append = 10`, returns `NoProgress`
  without growing the canvas (`src/stitch.rs:286-294`).
- **Horizontal drift / multi-axis scroll.** Filtered by `|tx| > 12 px`
  and a 0.12-magnitude geometry-drift cap, so any frame that has
  significant horizontal pan is rejected. Horizontal scrolling is
  unsupported (README §Limitations).
- **Direction reversal.** ORB enforces `ty > 1`. The README explicitly
  states only `col-sample` supports reverse scrolling.
- **Frame size change.** `estimate_orb_offset` returns `None` if
  dimensions differ (`src/stitch.rs:638-640`).
- **Feature-poor frames** (flat / line-only content). ORB fails the
  keypoint threshold; the template-matching fallback handles these.
  Test `opencv_orb_falls_back_to_template_on_low_feature_frames`
  (`src/stitch.rs:1395-1419`).
- **Duplicate frames.** Pre-stitch signature compare in
  `session.rs:226-239` skips identical frames so the stitcher does not
  waste an ORB cycle while the user is idle.
- **Static UI chrome / scrollbars.** Mitigated by the
  top/bottom/side mask (`ORB_*_IGNORE_RATIO`,
  `src/stitch.rs:781-801`).
- **Fixed headers/footers.** README §Limitations: not handled; will
  be repeatedly stitched. User must crop the region.

## Optimizations

- **Anchor frame preservation (ORB only).** Avoids drift accumulation
  by aligning every candidate against the *initial* frame instead of
  the previous one. Trades robustness on long scrolls (overlap might
  vanish) for accuracy (no error accumulation), and the relaxed/
  template fallbacks recover when the anchor distance gets large.
  `src/stitch.rs:277, 318-331`.
- **Capped ORB feature count.** `ORB_MAX_FEATURES = 1500` keeps the
  kNN brute-force cost bounded. `src/stitch.rs:31, 652`.
- **Hamming distance via BFMatcher.** ORB's 256-bit binary
  descriptors → `NORM_HAMMING` matcher → bitwise XOR + popcount per
  comparison; far cheaper than L2 over float descriptors.
- **Mask-restricted detection.** Skips chrome border regions so ORB
  doesn't spend descriptors on static UI. `build_feature_mask`
  `src/stitch.rs:781-801`.
- **OpenCL disabled.** `init_opencv_runtime` force-disables OpenCV's
  OpenCL backend (`OPENCV_OPENCL_RUNTIME=disabled` +
  `core::set_use_opencl(false)`), avoiding GPU-init stalls. CPU path
  only. `src/stitch.rs:49-58`.
- **Lowe's ratio + geometric pre-filter.** Cheap `|dx| ≤ 24,
  dy > 1` filter prunes matches before they reach RANSAC,
  shrinking RANSAC's inlier-count work.
  `src/stitch.rs:683-707`.
- **Affine-partial-2D (4 DOF) instead of full homography (8 DOF).**
  Fewer DOF → fewer RANSAC samples needed and much stricter geometry
  validation; appropriate for a 1-D scroll. `src/stitch.rs:714-723`.
- **Predicted-offset search ordering (template fallback).**
  `predict_offset_iter` emits `[p, p+1, p-1, p+2, p-2, ...]` so the
  NCC sweep starts at last frame's offset and spirals outward — early
  exits on score ≥ 0.95. `src/stitch.rs:487-506, 1112-1126`.
- **Duplicate-frame signature gate.** Avoids invoking the matcher
  while the user isn't scrolling. `src/session.rs:226-239`.
- **`rayon` parallelism.** Used by the experimental FAST/HNSW path for
  per-descriptor kNN; not used by the default ORB path
  (OpenCV's BFMatcher is internally parallelized).
  `src/stitch.rs:359-390`.
- **Release profile tuned for size:** `opt-level = "z"`, `lto = true`,
  `codegen-units = 1`, `strip = true`, `panic = "abort"`.
  `Cargo.toml:21-27`. Note: `opt-level = "z"` optimizes for size, not
  speed — a curious choice for a CV-heavy workload.

### Anti-optimizations / sharp edges

- `gray_to_mat` (`src/stitch.rs:767-779`) and `rgba_to_gray`
  (`src/stitch.rs:623-629`) iterate pixel-by-pixel through safe
  accessors instead of using `Mat::from_slice` / direct row pointers;
  for large frames this is an avoidable `O(W·H)` constant factor.
- Each successful append reallocates the entire canvas
  (`RgbaImage::new(W, H + Δ)` followed by two `copy_from` calls). For
  N frames of comparable height, total copy work is `O(W · H · N²)`.
  No `Vec`-style amortized growth.
- Stitching is **paste-only**: no seam blending, no feathering, no
  sub-pixel interpolation. The ORB `ty` value is `round()`-ed to an
  integer at `src/stitch.rs:519`, so any sub-pixel scroll is lost.

## Source References

- `learn-projects/wayscrollshot/src/cli.rs:7-21` — `Algorithm` enum,
  `OpenCvOrb` marked `#[default]`, CLI value name `opencv-orb`.
- `learn-projects/wayscrollshot/src/capture.rs:10-91` — `slurp` region
  selection and `grim`-based PNG capture loop.
- `learn-projects/wayscrollshot/src/session.rs:182-204` — `MatchConfig`
  for `OpenCvOrb` (`min_overlap=120`, `accept_diff=3.5`,
  `min_append=10`).
- `learn-projects/wayscrollshot/src/session.rs:215-241` — capture-and-
  dispatch loop, duplicate-signature suppression, `push_frame` call.
- `learn-projects/wayscrollshot/src/stitch.rs:22-46` — ORB tuning
  constants (`ORB_MAX_FEATURES=1500`, `ORB_MIN_KEYPOINTS=80`,
  `ORB_MIN_MATCHES=24`, `ORB_MIN_INLIERS=18`, `ORB_MAX_DX=12`,
  `ORB_MAX_GEOMETRY_DRIFT=0.12`, ROI ignore ratios).
- `learn-projects/wayscrollshot/src/stitch.rs:49-58` —
  `init_opencv_runtime`: disables OpenCV OpenCL.
- `learn-projects/wayscrollshot/src/stitch.rs:226-316` —
  `Stitcher::new` / `push_frame`: outcome dispatch, anchor preservation,
  canvas append.
- `learn-projects/wayscrollshot/src/stitch.rs:512-534` —
  `find_offset_opencv_orb`: primary → relaxed → template fallback.
- `learn-projects/wayscrollshot/src/stitch.rs:536-571` — relaxed-overlap
  retry and template fallback wiring.
- `learn-projects/wayscrollshot/src/stitch.rs:631-765` —
  `estimate_orb_offset`: the ORB+RANSAC core; ratio 0.78, ratio test,
  `estimate_affine_partial_2d` with `RANSAC, 3 px, 2000 iters, 0.99`.
- `learn-projects/wayscrollshot/src/stitch.rs:767-779` — `gray_to_mat`
  per-pixel copy.
- `learn-projects/wayscrollshot/src/stitch.rs:781-812` —
  `build_feature_mask` / `content_roi`: ORB attention mask.
- `learn-projects/wayscrollshot/src/stitch.rs:820-904` —
  `find_offset_template_content`: NCC fallback inside the content ROI.
- `learn-projects/wayscrollshot/src/stitch.rs:906-1012` —
  `ncc_score` / `ncc_score_region`: normalized cross-correlation.
- `learn-projects/wayscrollshot/src/stitch.rs:1014-1047` —
  `overlap_mean_abs_diff`: post-match verification metric for the
  NCC fallback.
- `learn-projects/wayscrollshot/src/stitch.rs:1112-1126` —
  `predict_offset_iter`: spiral search ordering around the predicted
  offset.
- `learn-projects/wayscrollshot/src/stitch.rs:1349-1445` — ORB-specific
  unit tests covering happy path, anchor preservation after a bad
  frame, low-feature template fallback, and relaxed-overlap recovery.
- `learn-projects/wayscrollshot/Cargo.toml:6-19` — dependency set
  (`opencv` features: `calib3d`, `features2d`, `imgproc`).
