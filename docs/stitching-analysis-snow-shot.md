# snow-shot — Scrolling Screenshot Stitching Algorithm

## Summary

snow-shot ("长截图" / scrolling screenshot) is a Tauri v2 app: a React/TypeScript
front-end drives a Rust back-end that does all of the heavy lifting. The default
(and only) stitcher is a **FAST corner detector + custom row/column-mean
descriptor + HNSW approximate nearest neighbour (ANN) feature-matching pipeline**
over a possibly down-scaled greyscale version of each freshly captured frame,
followed by histogram-mode offset voting and a non-blended (last-write-wins)
canvas paste. The whole crate lives under
`src-tauri/src-crates/app-scroll-screenshot-service/`.

## Pipeline

```
                 user scrolls (front-end JS)
                            │
   ┌────────────────────────┴───────────────────────────┐
   │ scrollScreenshotTool/index.tsx (React)              │
   │   • throttle(32 ms) + debounce(256 ms) on each wheel│
   │   • calls scroll_screenshot_capture(rect, dir)      │
   │   • debounce(100 ms) → scroll_screenshot_handle_image│
   └────────────────────────┬───────────────────────────┘
                            │ Tauri IPC
                            ▼
   ┌──────────────────────────────────────────────────────┐
   │ ScrollScreenshotCaptureService  (capture)            │
   │   monitor_list.capture_region(rect, Rgba8, hdr_corr) │
   │ ─────────────────────────────────────────────────────│
   │ ScrollScreenshotImageService  (FIFO of DynamicImage) │
   │   push_image(img, ScrollImageList::Top|Bottom)       │
   │ ─────────────────────────────────────────────────────│
   │ ScrollScreenshotService::handle_image                │
   │   1. to_luma8 + (optional) fast_image_resize         │
   │   2. corners_fast12 (fallback fast9)                 │
   │   3. compute_descriptor() per corner (rayon par_iter)│
   │   4. HNSWIndex::search vs. previous-edge index       │
   │      → (dx,dy) per corner, axis-locked               │
   │   5. mode-of-offsets vote (dominant > 2× runner-up)  │
   │   6. crop new strip (overlap = side/2 − |delta|)     │
   │      → push to top_image_list / bottom_image_list    │
   │   7. rebuild edge HNSW index lazily (min_size_delta) │
   │ ─────────────────────────────────────────────────────│
   │ export()                                             │
   │   • allocate RGBA8 canvas of full size               │
   │   • paste bottom_list ascending, then top_list       │
   │     descending — last write wins (no blending)       │
   └──────────────────────────────────────────────────────┘
```

Input per tick: one freshly captured RGBA8 region (same `(W,H)` as the
user-drawn selection). Output: an updated edge-position thumbnail + once
finished, a single PNG/RGBA8 long image.

## Algorithm: FAST + axial mean descriptor + HNSW ANN matching

### Step 1 — Capture & enqueue
Tauri command `scroll_screenshot_capture`
(`src-crates/tauri-commands/scroll-screenshot/src/lib.rs:45`) grabs the user-drawn
rectangle through `MonitorList::capture_region` (scap-like backend, RGBA8, optional
HDR/color-filter correction) and pushes a `DynamicImage` plus a `ScrollImageList`
tag (`Top`/`Bottom` — which side of the canvas this scroll was towards) into the
`VecDeque`-backed `ScrollScreenshotImageService`.

The front-end calls `scrollScreenshotCapture` on every wheel tick (throttled
32 ms + 256 ms trailing debounce, `index.tsx:421-433`) and a separate
`handleCaptureImageListDebounce` (100 ms, `index.tsx:374-376`) drains the queue
by repeatedly calling `scroll_screenshot_handle_image`. Capture and processing are
intentionally decoupled so the wheel never blocks on stitching.

### Step 2 — Greyscale + optional down-scale
`ScrollScreenshotService::get_gray_image`
(`scroll_screenshot_service.rs:321`):

- `image.to_luma8()` first (BT.601-ish weighting).
- If `image_scale < 1.0`, **only the scroll-perpendicular axis** is rescaled
  with `fast_image_resize` using `ResizeAlg::Nearest`. For vertical scrolling
  that means width is shrunk but height stays full-resolution — the matching
  axis keeps pixel precision.
- The scale is derived once in `init_image_size` (line 278) from frontend
  parameters `sample_rate`, `min_sample_size`, `max_sample_size`. With
  defaults (`sampleRate=1`, `min/maxSide=128`,
  `src/constants/appSettings.ts:259-266`) and any non-trivial selection width,
  `target_side = clamp(W*1, 128, 128) = 128`, so vertical captures actually
  get scaled down to **128 px wide × H tall**. This is the *default* config.

### Step 3 — FAST corner detection
`get_corners` (`:392`) calls `imageproc::corners::corners_fast12` with the
configured `corner_threshold` (default `imageFeatureThreshold = 24`).
The first frame decides the regime: if FAST-12 yields >200 corners,
FAST-12 sticks for the whole session, otherwise it falls back to
FAST-9 once (`enable_corner_fast12 = Some(false)`). Output is `Vec<ScrollOffset>`
of `(x, y)`.

### Step 4 — Custom 1-D mean descriptor
`compute_descriptor` (`:143`) builds a length-`patch_size` (default
`imageFeatureDescriptionLength = 28`) `Vec<f32>` per corner consisting of
two halves:

| Half | Content |
| ---- | ---- |
| First `P/2` entries | Mean luma of a **row** through the corner, sampled at stride 2 across `P` pixels horizontally |
| Last `P/2` entries | Mean luma of a **column** through the corner, sampled at stride 2 across `P` pixels vertically |

It is therefore **not** a 2-D patch but a pair of 1-D marginal-mean signatures.
Each descriptor entry is a single averaged grey value normalised to `[0,1]`.
Boundary pixels are skipped (no padding). All descriptors for one frame are
built in parallel via `rayon::par_iter`
(`get_descriptors`, `:310`).

### Step 5 — Approximate nearest neighbour (HNSW) match against the edge
The service keeps **two** edge indices: `top_image_ann_index` (descriptors
sitting at the very top of the accumulated canvas) and
`bottom_image_ann_index` (the very bottom). Each is a `ScrollIndex` wrapping
a `hora::index::hnsw_idx::HNSWIndex<f32, usize>` with `ef_search=24`/`ef_build=12`
(per-index defaults in `ScrollIndex::new`).

For each descriptor of the new frame, `get_offsets` (`:585`) does
`ann_index.search(descriptor, 1)`, computes Euclidean distance against the
stored descriptor, then accepts the match only when:

1. `dist < 0.1` (hard distance threshold), and
2. The motion is purely along the scroll axis — for vertical scroll `dx == 0`
   is required, else the match is discarded (`:622-634`). This is a very
   strict constraint that only works because the descriptor itself is 1-D
   axis-aligned and the down-scaling keeps the scroll axis at full resolution.
3. The offset `diff` does not point past the opposite edge (`min_diff`
   sanity guard, `:597-602`). Crossings are counted, and if >72% of corners
   trip this guard the frame is declared "no change" (`is_origin = true`,
   `:654-656`), which is how snow-shot detects "user reached the bottom of
   the page and the screenshot is the same as before".

### Step 6 — Mode-of-offsets vote (dominant shift)
Surviving `(diff, …)` tuples are histogrammed by exact integer offset
(`:663-674`). Two acceptance gates (`:705-711`):

- The winning bucket must have at least `corners/10` votes.
- It must be ≥ 2× the runner-up.

If either gate fails, the frame is dropped (`return (None, false, …)`).
This is the only "RANSAC-like" robustness step — there is no geometric model
fit beyond "pure-translation along one axis".

### Step 7 — Rollback / wrong-direction fallback
If matching against the user-intended side fails and `try_rollback` is true
(default), `handle_image` re-runs steps 5-6 against the *other* edge's index
(`:819-846`). This handles users who scroll up then down (or vice versa).

### Step 8 — Crop strip & append
`push_image` (`:515`) maps the descriptor match into a canvas-space offset and
computes `edge_position` (signed: positive ⇒ beyond bottom edge, negative ⇒
above top edge). It then calls `add_index` (`:463`):

- The new visible strip is `delta_size = edge_position - current_side_size`.
- An **overlap allowance** of `image_side/2 − |delta|` extra pixels is kept on
  the strip so that downstream paste re-covers half-frame of context
  (`:492-499`). This is also why `export()` later pastes with a negative
  `overlay_size` offset.
- The cropped strip (`image.crop_imm(...)`, `:503-508`) is appended to
  `top_image_list` or `bottom_image_list`, and `top_image_size`/`bottom_image_size`
  is increased.

### Step 9 — Lazy edge-index rebuild
`add_index` rebuilds the edge HNSW only when the running distance from the
current edge has exceeded `min_size_delta` (frontend passes `ceil(side*0.8)`,
i.e. **80% of the selection's scroll dimension**, `index.tsx:469-471`). When
that triggers, `build_index` (`:421`) constructs a fresh `HNSWIndex`
(`ef_search=32`, `ef_build=16`) populated with **this frame's** descriptors and
remembers its `position` so subsequent `(diff, position)` translations stay
consistent. Note: build params here differ from the constructor defaults.

### Step 10 — Final compositing (`export`, `:876`)
- Allocate a single contiguous RGBA8 buffer of `total_width * total_height * 4`
  bytes via `Vec::with_capacity` + `set_len` (uninitialised, then fully
  overwritten — safe in practice because every output pixel is covered by
  the union of crops).
- Paste **bottom_image_list** in insertion order via `overlay_image`
  (`src-crates/app-utils/src/lib.rs:631`), which is a memcpy-per-row using
  `std::ptr::copy_nonoverlapping` parallelised over rows with rayon.
- Then paste **top_image_list** in reverse, *over* the bottom data.
- Effective seam policy: **last write wins** (no alpha blend, no feathering,
  no Poisson/multi-band). The `overlay_size` overlap means the freshest crop
  always covers the half-frame stale region of the previous crop.

Result: `DynamicImage::ImageRgba8` returned to TS, encoded as PNG (`Fast`
deflate, Paeth filter) for IPC, or shared via `SharedBuffer` on Windows.

## Time Complexity

Let
- `W` = capture width, `H` = capture height
- `S` = scroll side (`H` for vertical, `W` for horizontal)
- `Wd, Hd` = down-scaled dimensions (default vertical: `Wd=128`, `Hd=H`)
- `C` = number of FAST corners per frame
- `P` = descriptor patch size (default 28, descriptor length `2·⌊P/2⌋ = 28`)
- `N` = corners in the current edge index (≈ `C`)
- `R` = rayon thread count
- `K = ef_search` for HNSW (24 by default in matching index)

| Step | Cost | Notes |
| ---- | ---- | ---- |
| Capture region (OS) | platform-specific | scap-style, not counted |
| `to_luma8` | `Θ(W·H)` | single linear pass |
| `fast_image_resize` (nearest) | `Θ(Wd·Hd)` | skipped when `image_scale ≥ 1`; with defaults this is ≈ `Θ(128·H)` |
| `corners_fast12/9` (`imageproc`) | `Θ(Wd·Hd)` expected, ≈ `Θ(Wd·Hd · 16)` worst case | FAST tests a 16-pixel ring per pixel; cheap when the contiguous-arc early-out kicks in |
| `get_descriptors` (rayon) | `Θ(C · P / R)` | each descriptor scans `P/2 + P/2 = P` samples |
| HNSW build (rebuild only) | `Θ(N · log N · ef_build)` ≈ `Θ(N log N · 16)` | only when `min_size_delta` exceeded (every ~80% of `S` worth of scroll); ANN add+build under serial loop |
| HNSW search (per query, rayon) | `Θ(log N · ef_search)` ≈ `Θ(K log N)` | `C` queries → `Θ(C · K log N / R)` |
| Distance check / axis guard | `Θ(P)` per query | inside the par_iter |
| Mode-of-offsets vote | `Θ(C)` | HashMap insertion |
| `image.crop_imm` strip | `Θ(W · stripH)` where `stripH = side/2 + |delta|` | full RGBA8 row copies (DynamicImage clones the buffer) |
| `export()` final paste | `Θ(W · Htotal)` | `total_width · total_height` row memcpys, parallel over rows |

**Total per stitched frame (matching path)**:
`O(W·H + C · (P + K · log N) / R)`.
With defaults (`W·H ≈ selection`, `C≈few-hundred`, `P=28`, `K=24`,
`N ≈ C`, vertical → `Wd=128`) the `W·H` greyscale-conversion term clearly
dominates everything else; corner detection on the *scaled* image is the next
biggest constant.

**Total on rebuild frames**: add `O(N log N · ef_build)`.

**Final export**: `O(W · Htotal)` one-shot memcpy.

There is **no template/template-NCC sliding window** at all, so the usual
"search range × overlap" factor does not appear — ANN matching turns the
correspondence search into log-cost lookups against pre-indexed features
keyed only on a 1-D mean signature.

## Space Complexity

Let `F` = number of stitched frames retained, `Htotal = top_image_size + bottom_image_size`.

| Buffer | Size | Lifetime |
| ---- | ---- | ---- |
| Original RGBA8 capture per tick | `4·W·H` | dropped after `handle_image` returns |
| Greyscale (after down-scale) | `Wd·Hd` | per-frame, dropped after build_index call |
| FAST corners | `Θ(C)` `(i32,i32)` | per-frame |
| Descriptors | `Θ(C · P · 4)` bytes (`Vec<Vec<f32>>`) | per-frame; copied into edge index on rebuild |
| `top_image_ann_index` + `bottom_image_ann_index` | `Θ(N · P · 4)` + HNSW graph (`Θ(N · M · 8)` w/ default M; hora’s overhead) | persists |
| `top_image_list` / `bottom_image_list` | `Σ 4·W·stripH_i ≈ 4·W·Htotal · (≈1.5)` (overlap allowance) | persists until clear/export |
| `final_image` (export) | `4·W·Htotal` | one-shot, returned to caller |
| Per-frame queue (`VecDeque`) | up to a few `DynamicImage`s `4·W·H` each | bounded by user scroll rate |

**Total (steady state)**: `O(W·Htotal)` dominated by the strip list plus the
final canvas, with an extra `O(N · P)` constant for the two edge indices.

## Edge Cases

- **No-match / few corners**: empty `image_corners` early-returns
  `(None, false, …)` — frame silently dropped, front-end sees "no change".
  Same outcome if dominant bucket has <`C/10` votes or runner-up too close.
- **End of page detected**: `min_diff` guard counts how many corners would
  match *past* the opposite edge; if >72% of them do, `is_origin = true` and
  the front-end is told "no change" (`ScrollScreenshotCaptureResult` type
  `"no_change"`).
- **Direction reversal**: handled by maintaining two edge indices and the
  `try_rollback` fallback path (default `true`). If a frame fails against the
  intended edge it is re-tried against the other.
- **Cross-axis motion**: by design rejected — `dx != 0` (vertical scroll) or
  `dy != 0` (horizontal scroll) immediately disqualifies a candidate match
  (`:622-634`). The pipeline is a *pure-translation* stitcher; it cannot
  recover diagonal scrolls or rotations.
- **Resolution change mid-session**: if a later frame's `(W,H)` differs from
  the first, `handle_image` early-returns (`:742-744`). No re-init.
- **macOS logical vs physical pixels**: the Tauri command scales the
  selection rect by `1 / window.scale_factor()` before calling the native
  capture (`tauri-commands/scroll-screenshot/src/lib.rs:59-73`).
- **First frame**: bootstraps the top index from itself (`:757-789`), so the
  initial "edge" is the whole frame.
- **Identical frames stacking up**: capture and handle are decoupled —
  `scroll_screenshot_image_service` is a `VecDeque` that can absorb bursts
  while stitching catches up.

## Optimizations

- **rayon**: descriptor computation (`get_descriptors`), HNSW query loop
  (`get_offsets`), final image paste row loop (`overlay_image_ptr`), and
  the `fast_image_resize` crate are all parallel.
- **fast_image_resize** with the `rayon` feature and `ResizeAlg::Nearest` —
  cheap because perceptual quality of the resized image is irrelevant; only
  the FAST corner pattern needs to survive.
- **One-axis-only down-scaling**: scroll axis is always kept at native
  resolution so per-pixel offsets are still pixel-accurate after stitching.
- **FAST-12 → FAST-9 auto-fallback**: probe the first frame; FAST-12 is faster
  but produces fewer corners — fall back to FAST-9 once if the image is
  texture-poor (`enable_corner_fast12` latches for the whole session).
- **HNSW (hora)** instead of brute-force descriptor matching — turns
  per-corner matching into `O(log N · ef_search)` and avoids any sliding
  template-window cost.
- **Lazy edge-index rebuild** via `min_size_delta = ceil(side * 0.8)`: only
  rebuild when the edge has drifted enough, amortising the HNSW build cost.
- **Capture / handle decoupling**: two separate Tauri commands and two Mutex
  states (`ScrollScreenshotImageService` queue vs. processing service) so
  the wheel/throttle path never blocks on CPU work.
- **`crop_imm` to drop redundant overlap before storage**: only the new strip
  (plus a half-frame overlap allowance) is retained per tick, not the whole
  frame.
- **Final paste is raw `ptr::copy_nonoverlapping` per row**, no per-pixel
  blending — keeps `export()` to a single linear memcpy pass.
- **PNG encoding uses `Fast` deflate + Paeth filter** for the thumbnail and
  IPC path; only the explicit "save as PNG" path uses default compression.
- **Windows `SharedBuffer` fast-path** to hand the final RGBA8 directly to the
  webview without round-tripping through PNG
  (`tauri-commands/scroll-screenshot/src/lib.rs:299-329`).

No SIMD intrinsics, no GPU/wgpu, no OpenCV, no image pyramids beyond the one
fixed down-scale, no sub-pixel refinement (offsets are integer-only, and
intentionally — both the descriptor sampling stride and FAST quantise to
integer pixels).

## Source References

- `src-tauri/src-crates/app-scroll-screenshot-service/Cargo.toml:1` — crate deps:
  `image`, `imageproc`, `rayon`, `fast_image_resize`, `hora` (HNSW).
- `src-tauri/src-crates/app-scroll-screenshot-service/src/scroll_screenshot_service.rs:60`
  — `ScrollIndex` (HNSW + corners + descriptors per edge).
- `:82` — `ScrollImage { image, overlay_size }` strip type.
- `:87` — `ScrollScreenshotService` aggregate state.
- `:143` — `compute_descriptor` (row-mean + column-mean signature).
- `:202` — Euclidean distance helper.
- `:246` — `init` (frontend-tunable params: thresholds, sample sizes, rollback).
- `:278` — `init_image_size` (computes `image_scale` from `sample_rate` /
  `min_sample_size` / `max_sample_size`).
- `:321` — `get_gray_image` (luma + nearest-neighbour resize on one axis).
- `:358` — `get_crop_region` (carves the new strip from the captured frame).
- `:392` — `get_corners` (FAST-12 with FAST-9 fallback after probing).
- `:421` — `build_index` (rebuilds the per-edge HNSW with `ef_search=32`,
  `ef_build=16`).
- `:463` — `add_index` (decides whether to rebuild, computes overlap allowance,
  crops the strip).
- `:515` — `push_image` (translates feature offset to canvas-space edge
  position and dispatches to top/bottom list).
- `:585` — `get_offsets` (HNSW query + axis-lock + mode-of-offsets voting +
  72% min_diff "end of page" detector).
- `:726` — `handle_image` (top-level per-frame entry; first-frame bootstrap
  and `try_rollback` fallback live here).
- `:876` — `export` (final RGBA8 canvas assembly via row memcpy).
- `src-tauri/src-crates/app-scroll-screenshot-service/src/scroll_screenshot_capture_service.rs:9`
  — capture-side monitor handle cache.
- `src-tauri/src-crates/app-scroll-screenshot-service/src/scroll_screenshot_image_service.rs:15`
  — `VecDeque`-backed FIFO between capture and handler.
- `src-tauri/src-crates/tauri-commands/scroll-screenshot/src/lib.rs:18` —
  `scroll_screenshot_init` IPC.
- `:45` — `scroll_screenshot_capture` (calls `MonitorList::capture_region`,
  pushes onto the FIFO; macOS scale_factor handling at `:59-73`).
- `:114` — `scroll_screenshot_handle_image` (drives `handle_image`, generates
  thumbnail + appends edge/size metadata; sentinel bytes `1`/`2`).
- `:278` — `scroll_screenshot_get_image_data` (final export + Windows
  `SharedBuffer` fast-path).
- `src-tauri/src-crates/app-utils/src/lib.rs:596` — `overlay_image_ptr` (raw
  parallel row memcpy used by `export`).
- `:631` — `overlay_image` (safe wrapper invoked from `export`).
- `src-tauri/src/scroll_screenshot.rs:14` — Tauri command thin wrappers
  registered on the app.
- `src/commands/scrollScreenshot.ts:19` — TS bindings + result-type parsing.
- `src/pages/draw/components/drawToolbar/components/tools/scrollScreenshotTool/index.tsx:378`
  — `captureImageCore` (wheel-driven driver, 32 ms throttle / 256 ms debounce).
- `:374` — `handleCaptureImageListDebounce` (100 ms drain of the FIFO).
- `:443-481` — `init`, which passes `ceil(side * 0.8)` as `min_size_delta`,
  forcing edge-index rebuilds roughly every 80% of the selection side.
- `src/constants/appSettings.ts:259-266` — **default knob values**
  (`tryRollback=true`, `imageFeatureThreshold=24`, `min/maxSide=128`,
  `sampleRate=1`, `imageFeatureDescriptionLength=28`).
