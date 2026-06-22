# rollshot-vision regionFeatures v0 (Sub-project 2) — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement `RealAutomationHost::region_features` as a deterministic numeric sanity filter: `regionFeatures({region, limit})` returns a single `RegionFeatures` (dominant color + edge density) describing the whole requested rect.

**Architecture:** Mirror SP1's templateMatch contract — expensive per-region computation runs in a `prepare_region_features` step **outside** QuickJS; the QuickJS callback only resolves the query region to a canonical `PixelRect` key, looks it up in a prepared cache, and truncates. Features are computed per-region from the `VisualIndex`'s already-cached grayscale (edge density) and RGBA source (dominant color). No full-image edge map, no capability-API change.

**Tech Stack:** Rust, `image` 0.25 (`RgbaImage` / `GrayImage`), `rollshot-automation` capability types, `rollshot-automation-rquickjs` (`QuickJsExecutor`, dev-dep, integration only).

## Global Constraints

Every task implicitly includes these (values copied verbatim from the spec / AGENTS.md):

- Crate keeps `#![forbid(unsafe_code)]` (already set in `lib.rs`).
- **No capability-API change.** `rollshot_automation::RegionFeatures { bounds: ImageRect, dominant_rgba: [u8;4], edge_density: f32 }` and `RegionFeaturesQuery { region, limit }` are used as-is; do not edit `rollshot-automation`.
- v0 result length is always **0 or 1**. `limit` never controls tiling/result count. No subregion splitting, no connected components, no V2 fields.
- `QUANTIZE_STEP` **must divide 256**; bin center = `bin_index * QUANTIZE_STEP + QUANTIZE_STEP/2`; tie-break = lowest bin index.
- `edge_density` denominator is fixed at `(w-1)*(h-1)`; use **`u64` accumulators**; width<2 or height<2 → `0.0` (no panic, no divide-by-zero).
- `RegionFeatures.bounds` returns the **clipped measured rect**, not the raw requested bounds.
- Prepared lookup uses a **canonical `PixelRect` key**, never raw `Region` equality.
- Reuse existing `CapabilityError` codes only: `invalid_query`, `non_finite_region`, `empty_region`, `region_too_large`, `vision_index_unavailable`, `LimitExceeded`. **No new `VisionError` variant or error code.**
- All runtime diagnostics use `tracing` with target `"rollshot::vision::region_features"` and structured fields. No `println!` / `eprintln!` / `dbg!`.
- Deterministic only: no `Date`, no randomness.
- Branch: `feat/rollshot-vision-region-features` (already checked out).
- Verification commands prefixed with `rtk`. Commit messages end with:
  `Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>`

---

### Task 1 (PR1): `region_features.rs` pure functions

**Files:**
- Create: `crates/rollshot-vision/src/region_features.rs`
- Modify: `crates/rollshot-vision/src/lib.rs` (add `mod region_features;`)
- Test: inline `#[cfg(test)] mod tests` in `region_features.rs` (matches `index.rs` / `rect.rs` style)

**Interfaces:**
- Consumes: `crate::rect::{PixelRect, MAX_SEARCH_AREA}`; `image::{RgbaImage, GrayImage}`.
- Produces (used by Task 2):
  - `pub(crate) const QUANTIZE_STEP: u32 = 16;`
  - `pub(crate) const EDGE_THRESHOLD: u16 = 32;`
  - `pub(crate) const MAX_REGION_FEATURES_AREA: u64 = crate::rect::MAX_SEARCH_AREA;`
  - `pub(crate) fn dominant_rgba(image: &RgbaImage, rect: PixelRect) -> [u8; 4]`
  - `pub(crate) fn edge_density(gray: &GrayImage, rect: PixelRect) -> f32`

- [ ] **Step 1: Create the module skeleton and wire it into `lib.rs`**

Add to `crates/rollshot-vision/src/lib.rs` after the `mod rect;`/`pub mod rect;` line group (keep alphabetical-ish ordering near the other `mod` lines):

```rust
mod region_features;
```

Create `crates/rollshot-vision/src/region_features.rs`:

```rust
//! Deterministic, per-region numeric features used as a runtime sanity filter.
//! Pure functions over `VisualIndex` data; no host state, no QuickJS, no alloc
//! of new images. Computed inside `prepare_region_features` (outside QuickJS).

use image::{GrayImage, RgbaImage};

use crate::rect::PixelRect;

/// RGB quantization step. MUST divide 256 (256 / 16 = 16 bins per channel).
pub(crate) const QUANTIZE_STEP: u32 = 16;

/// Per-pixel combined-gradient threshold (`|dx| + |dy|`) for counting an edge.
pub(crate) const EDGE_THRESHOLD: u16 = 32;

/// Area cap for a regionFeatures query (reuse the template search-area cap).
// `#[allow(dead_code)]` is removed in PR2 once the host consumes these; in PR1
// they have no non-test consumer, so the lib-target build would flag dead_code.
#[allow(dead_code)]
pub(crate) const MAX_REGION_FEATURES_AREA: u64 = crate::rect::MAX_SEARCH_AREA;

/// Dominant quantized color of `rect`, returned as the winning bin's center.
/// Alpha is fixed at 255 (SP2 assumes screenshot-like opaque input).
#[allow(dead_code)] // removed in PR2 when RealAutomationHost consumes it
pub(crate) fn dominant_rgba(_image: &RgbaImage, _rect: PixelRect) -> [u8; 4] {
    todo!("implement in Step 4")
}

/// Fraction of in-rect pixels (with both a right and a down neighbor) whose
/// combined gradient exceeds `EDGE_THRESHOLD`. Range [0, 1]; 0.0 if rect is
/// narrower/shorter than 2 px.
#[allow(dead_code)] // removed in PR2 when RealAutomationHost consumes it
pub(crate) fn edge_density(_gray: &GrayImage, _rect: PixelRect) -> f32 {
    todo!("implement in Step 8")
}
```

> The `#[allow(dead_code)]` attributes sit above each item's signature and are
> preserved when Steps 4 and 8 replace the function *bodies*. They are deleted in
> Task 2 Step 2 once the host references all three, keeping `-D warnings` green at
> every PR. `QUANTIZE_STEP` / `EDGE_THRESHOLD` need no attribute — the functions
> reference them.

- [ ] **Step 2: Write the failing tests for `dominant_rgba`**

Append to `region_features.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::rect::PixelRect;

    fn full(w: u32, h: u32) -> PixelRect {
        PixelRect { x: 0, y: 0, width: w, height: h }
    }

    #[test]
    fn dominant_of_solid_region_is_that_bins_center() {
        // (104,152,200) are already bin centers for QUANTIZE_STEP=16
        // (104/16=6 -> 6*16+8=104, 152->152, 200->200), so output == input rgb.
        let img = RgbaImage::from_pixel(8, 8, image::Rgba([104, 152, 200, 255]));
        assert_eq!(dominant_rgba(&img, full(8, 8)), [104, 152, 200, 255]);
    }

    #[test]
    fn dominant_picks_majority_color() {
        // Left 6 cols red, right 2 cols blue -> red wins.
        let mut img = RgbaImage::from_pixel(8, 4, image::Rgba([200, 40, 40, 255]));
        for y in 0..4 {
            for x in 6..8 {
                img.put_pixel(x, y, image::Rgba([40, 40, 200, 255]));
            }
        }
        // 200 -> bin 12 -> center 200; 40 -> bin 2 -> center 40.
        assert_eq!(dominant_rgba(&img, full(8, 4)), [200, 40, 40, 255]);
    }

    #[test]
    fn dominant_tie_breaks_to_lowest_bin_index() {
        // Half pixels color A (lower bin), half color B (higher bin), equal count.
        let mut img = RgbaImage::from_pixel(4, 2, image::Rgba([8, 8, 8, 255])); // bin 0 -> center 8
        for x in 2..4 {
            img.put_pixel(x, 0, image::Rgba([200, 200, 200, 255])); // bin 12 -> center 200
            img.put_pixel(x, 1, image::Rgba([200, 200, 200, 255]));
        }
        // 4 px at (8,8,8) bin index 0, 4 px at (200,200,200) higher index -> tie -> lowest wins.
        assert_eq!(dominant_rgba(&img, full(4, 2)), [8, 8, 8, 255]);
    }
}
```

- [ ] **Step 3: Run the `dominant_rgba` tests to verify they fail**

Run: `rtk cargo test -p rollshot-vision --lib region_features::tests::dominant`
Expected: FAIL — each test panics on `todo!("implement in Step 4")`.

- [ ] **Step 4: Implement `dominant_rgba`**

Replace the `dominant_rgba` body:

```rust
pub(crate) fn dominant_rgba(image: &RgbaImage, rect: PixelRect) -> [u8; 4] {
    let bins_per_channel = 256 / QUANTIZE_STEP; // 16
    let bin_count = (bins_per_channel * bins_per_channel * bins_per_channel) as usize;
    let mut histogram = vec![0u32; bin_count];

    for y in rect.y..rect.y + rect.height {
        for x in rect.x..rect.x + rect.width {
            let px = image.get_pixel(x, y).0;
            let rb = px[0] as u32 / QUANTIZE_STEP;
            let gb = px[1] as u32 / QUANTIZE_STEP;
            let bb = px[2] as u32 / QUANTIZE_STEP;
            let index = (rb * bins_per_channel + gb) * bins_per_channel + bb;
            histogram[index as usize] += 1;
        }
    }

    // Lowest index wins ties: `>` keeps the first (lowest) max.
    let mut best_index = 0usize;
    let mut best_count = 0u32;
    for (index, &count) in histogram.iter().enumerate() {
        if count > best_count {
            best_count = count;
            best_index = index;
        }
    }

    let bins = bins_per_channel as usize;
    let rb = (best_index / (bins * bins)) as u32;
    let gb = ((best_index / bins) % bins) as u32;
    let bb = (best_index % bins) as u32;
    let center = |bin: u32| (bin * QUANTIZE_STEP + QUANTIZE_STEP / 2) as u8;
    [center(rb), center(gb), center(bb), 255]
}
```

- [ ] **Step 5: Run the `dominant_rgba` tests to verify they pass**

Run: `rtk cargo test -p rollshot-vision --lib region_features::tests::dominant`
Expected: PASS (3 tests).

- [ ] **Step 6: Write the failing tests for `edge_density`**

Append inside `mod tests`:

```rust
    fn gray_from_fn(w: u32, h: u32, f: impl Fn(u32, u32) -> u8) -> GrayImage {
        GrayImage::from_fn(w, h, |x, y| image::Luma([f(x, y)]))
    }

    #[test]
    fn edge_density_of_solid_region_is_zero() {
        let g = gray_from_fn(8, 8, |_, _| 128);
        assert_eq!(edge_density(&g, full(8, 8)), 0.0);
    }

    #[test]
    fn edge_density_of_vertical_stripes_is_one() {
        // Alternating columns 0/255: every counted pixel has |dx|=255 >= threshold.
        let g = gray_from_fn(4, 4, |x, _| if x % 2 == 0 { 0 } else { 255 });
        assert_eq!(edge_density(&g, full(4, 4)), 1.0);
    }

    #[test]
    fn edge_density_below_threshold_is_zero() {
        // Constant small horizontal ramp with step < EDGE_THRESHOLD -> no edges.
        let step = (EDGE_THRESHOLD - 1) as u32;
        let g = gray_from_fn(4, 4, |x, _| (x * step).min(255) as u8);
        assert_eq!(edge_density(&g, full(4, 4)), 0.0);
    }

    #[test]
    fn edge_density_narrow_region_is_zero_no_panic() {
        let g = gray_from_fn(8, 8, |x, _| if x % 2 == 0 { 0 } else { 255 });
        assert_eq!(edge_density(&g, PixelRect { x: 0, y: 0, width: 1, height: 8 }), 0.0);
        assert_eq!(edge_density(&g, PixelRect { x: 0, y: 0, width: 8, height: 1 }), 0.0);
    }
```

- [ ] **Step 7: Run the `edge_density` tests to verify they fail**

Run: `rtk cargo test -p rollshot-vision --lib region_features::tests::edge_density`
Expected: FAIL — each panics on `todo!("implement in Step 8")`.

- [ ] **Step 8: Implement `edge_density`**

Replace the `edge_density` body:

```rust
pub(crate) fn edge_density(gray: &GrayImage, rect: PixelRect) -> f32 {
    if rect.width < 2 || rect.height < 2 {
        return 0.0;
    }
    let mut edge_count: u64 = 0;
    let mut counted: u64 = 0;
    let x_end = rect.x + rect.width - 1; // exclusive of last col (no right neighbor)
    let y_end = rect.y + rect.height - 1; // exclusive of last row (no down neighbor)
    for y in rect.y..y_end {
        for x in rect.x..x_end {
            let here = gray.get_pixel(x, y).0[0] as i16;
            let right = gray.get_pixel(x + 1, y).0[0] as i16;
            let down = gray.get_pixel(x, y + 1).0[0] as i16;
            let grad = (here - right).unsigned_abs() + (here - down).unsigned_abs();
            if grad >= EDGE_THRESHOLD {
                edge_count += 1;
            }
            counted += 1;
        }
    }
    if counted == 0 {
        0.0
    } else {
        edge_count as f32 / counted as f32
    }
}
```

- [ ] **Step 9: Run the full module tests to verify they pass**

Run: `rtk cargo test -p rollshot-vision --lib region_features`
Expected: PASS (7 tests).

- [ ] **Step 10: Format and lint**

Run: `rtk cargo fmt -p rollshot-vision` then `rtk cargo clippy -p rollshot-vision --all-targets -- -D warnings`
Expected: no diffs from fmt; clippy clean.

- [ ] **Step 11: Commit**

```bash
rtk git add crates/rollshot-vision/src/region_features.rs crates/rollshot-vision/src/lib.rs
rtk git commit -m "feat(vision): regionFeatures v0 dominant color + edge density

PR1/SP2: pure, deterministic per-region functions. dominant_rgba is a
quantized RGB histogram (QUANTIZE_STEP=16, bin-center output, lowest-bin
tie-break); edge_density is the fraction of in-rect pixels whose |dx|+|dy|
combined gradient exceeds EDGE_THRESHOLD, denominator (w-1)*(h-1), u64
accumulators, 0.0 for sub-2px rects. No host wiring yet.

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 2 (PR2): canonical-key prepare + cached-lookup callback

**Files:**
- Modify: `crates/rollshot-vision/src/rect.rs` (add `Hash` to `PixelRect` derive)
- Modify: `crates/rollshot-vision/src/host.rs` (key type, prepared cache, stored dims, prepare method, real `region_features` impl)
- Test: inline `#[cfg(test)] mod tests` in `host.rs` (extend the existing module)

**Interfaces:**
- Consumes (from Task 1): `crate::region_features::{dominant_rgba, edge_density, MAX_REGION_FEATURES_AREA}`; `crate::rect::{PixelRect, region_to_pixel_rect}`; `rollshot_automation::{RegionFeatures, RegionFeaturesQuery, CapabilityError, Region}`; `rollshot_image_document::ImageRect`; `crate::index::VisualIndex`.
- Produces (used by Task 3, via the public `AutomationHost` impl + new method):
  - `pub fn prepare_region_features(&mut self, index: &VisualIndex, query: &RegionFeaturesQuery) -> Result<(), CapabilityError>`
  - `<RealAutomationHost as AutomationHost>::region_features(&mut self, RegionFeaturesQuery) -> Result<Vec<RegionFeatures>, CapabilityError>` (real, replacing the stub).

> **Why the host stores image dimensions:** the QuickJS callback receives only the
> query — it has no `VisualIndex`. To canonicalize `query.region -> PixelRect`
> (so `Full` and an equivalent rect collapse to one key, and f32/clip differences
> are erased), the callback needs the image size. The host records it at prepare
> time. This is the concrete realization of the spec's canonical-key contract.

- [ ] **Step 1: Add `Hash` to `PixelRect`**

In `crates/rollshot-vision/src/rect.rs`, change the `PixelRect` derive:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PixelRect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}
```

- [ ] **Step 2: Remove the PR1 `#[allow(dead_code)]` attributes, then add the key type, prepared cache, and stored dims to `host.rs`**

First, in `crates/rollshot-vision/src/region_features.rs`, delete the three `#[allow(dead_code)]` lines (above `MAX_REGION_FEATURES_AREA`, `dominant_rgba`, and `edge_density`) — the host now consumes all three.

Then, in `crates/rollshot-vision/src/host.rs`, extend the imports and types. Update the `use` block at the top:

```rust
use rollshot_automation::{
    AutomationHost, CapabilityError, LayoutQuery, LayoutRegion, OcrMatch, OcrQuery, RegionFeatures,
    RegionFeaturesQuery, TemplateMatch, TemplateMatchQuery,
};
use rollshot_image_document::ImageRect;

use crate::index::VisualIndex;
use crate::rect::{region_to_pixel_rect, PixelRect};
use crate::region_features::{dominant_rgba, edge_density, MAX_REGION_FEATURES_AREA};
use crate::template::{prepare_template_match as prepare_template_results, TemplateStore};
```

Add the key + prepared struct near `PreparedTemplateMatch`:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct RegionFeaturesKey {
    rect: PixelRect,
}

#[derive(Debug, Clone)]
struct PreparedRegionFeatures {
    key: RegionFeaturesKey,
    max_limit: u32,
    results: Vec<RegionFeatures>, // v0: always length 1
}
```

Extend the host struct (keep `#[derive(Debug, Default)]`):

```rust
#[derive(Debug, Default)]
pub struct RealAutomationHost {
    prepared_template_matches: Vec<PreparedTemplateMatch>,
    prepared_region_features: Vec<PreparedRegionFeatures>,
    image_dimensions: Option<(u32, u32)>,
}
```

- [ ] **Step 3: Write the failing tests**

Add these tests to the existing `#[cfg(test)] mod tests` in `host.rs`:

```rust
    fn checkerboard(size: u32) -> image::RgbaImage {
        image::RgbaImage::from_fn(size, size, |x, y| {
            let v = if (x + y) % 2 == 0 { 0 } else { 255 };
            image::Rgba([v, v, v, 255])
        })
    }

    #[test]
    fn unprepared_region_features_query_fails_explicitly() {
        let mut host = RealAutomationHost::new();
        let err = host
            .region_features(RegionFeaturesQuery { region: Region::Full, limit: 1 })
            .unwrap_err();
        assert_eq!(err, CapabilityError::Failed { code: "vision_index_unavailable" });
    }

    #[test]
    fn region_features_rejects_zero_limit() {
        let mut host = RealAutomationHost::new();
        let err = host
            .region_features(RegionFeaturesQuery { region: Region::Full, limit: 0 })
            .unwrap_err();
        assert_eq!(err, CapabilityError::InvalidInput { code: "invalid_query" });
    }

    #[test]
    fn prepared_region_features_round_trips_and_canonical_key_matches() {
        let index = VisualIndex::build(checkerboard(8)).unwrap();
        let mut host = RealAutomationHost::new();
        host.prepare_region_features(&index, &RegionFeaturesQuery { region: Region::Full, limit: 1 })
            .unwrap();

        // Full was prepared; an equivalent explicit full rect must hit the same key.
        let equivalent_full = Region::Rect {
            bounds: ImageRect { x: 0.0, y: 0.0, width: 8.0, height: 8.0 },
        };
        let out = host
            .region_features(RegionFeaturesQuery { region: equivalent_full, limit: 1 })
            .unwrap();
        assert_eq!(out.len(), 1);
        // Clipped measured bounds, not raw requested bounds.
        assert_eq!(out[0].bounds, ImageRect { x: 0.0, y: 0.0, width: 8.0, height: 8.0 });
        // Checkerboard -> every counted pixel is an edge.
        assert_eq!(out[0].edge_density, 1.0);
    }

    #[test]
    fn region_features_limit_over_prepared_max_is_limit_exceeded() {
        let index = VisualIndex::build(checkerboard(8)).unwrap();
        let mut host = RealAutomationHost::new();
        host.prepare_region_features(&index, &RegionFeaturesQuery { region: Region::Full, limit: 1 })
            .unwrap();
        let err = host
            .region_features(RegionFeaturesQuery { region: Region::Full, limit: 2 })
            .unwrap_err();
        assert_eq!(err, CapabilityError::LimitExceeded);
    }
```

- [ ] **Step 4: Run the tests to verify they fail**

Run: `rtk cargo test -p rollshot-vision --lib host::tests::region_features --lib host::tests::prepared_region_features --lib host::tests::unprepared_region_features`
Expected: FAIL — `prepare_region_features` does not exist (compile error) and the stub `region_features` returns `capability_unavailable`.

- [ ] **Step 5: Implement `prepare_region_features` and the real callback**

Add the prepare method inside `impl RealAutomationHost` (next to `prepare_template_match`):

```rust
    /// Expensive preparation. Call before entering `QuickJsExecutor`.
    pub fn prepare_region_features(
        &mut self,
        index: &VisualIndex,
        query: &RegionFeaturesQuery,
    ) -> Result<(), CapabilityError> {
        let started = Instant::now();
        let rect = region_to_pixel_rect(
            &query.region,
            index.width(),
            index.height(),
            MAX_REGION_FEATURES_AREA,
        )?;
        let key = RegionFeaturesKey { rect };
        let features = RegionFeatures {
            bounds: ImageRect {
                x: rect.x as f32,
                y: rect.y as f32,
                width: rect.width as f32,
                height: rect.height as f32,
            },
            dominant_rgba: dominant_rgba(index.image(), rect),
            edge_density: edge_density(index.gray(), rect),
        };
        self.image_dimensions = Some((index.width(), index.height()));
        self.prepared_region_features.retain(|prepared| prepared.key != key);
        self.prepared_region_features.push(PreparedRegionFeatures {
            key,
            max_limit: query.limit,
            results: vec![features],
        });
        tracing::debug!(
            target: "rollshot::vision::region_features",
            duration_ms = started.elapsed().as_millis() as u64,
            result_count = 1u64,
            "region features prepared"
        );
        Ok(())
    }
```

Replace the stub `region_features` in `impl AutomationHost for RealAutomationHost`:

```rust
    fn region_features(
        &mut self,
        query: RegionFeaturesQuery,
    ) -> Result<Vec<RegionFeatures>, CapabilityError> {
        if query.limit == 0 {
            return Err(CapabilityError::InvalidInput { code: "invalid_query" });
        }
        let (width, height) = self
            .image_dimensions
            .ok_or(CapabilityError::Failed { code: "vision_index_unavailable" })?;
        let rect = region_to_pixel_rect(&query.region, width, height, MAX_REGION_FEATURES_AREA)?;
        let key = RegionFeaturesKey { rect };
        let prepared = self
            .prepared_region_features
            .iter()
            .find(|prepared| prepared.key == key)
            .ok_or(CapabilityError::Failed { code: "vision_index_unavailable" })?;
        if query.limit > prepared.max_limit {
            return Err(CapabilityError::LimitExceeded);
        }
        Ok(prepared
            .results
            .iter()
            .take(query.limit as usize)
            .cloned()
            .collect())
    }
```

- [ ] **Step 6: Run the tests to verify they pass**

Run: `rtk cargo test -p rollshot-vision --lib host`
Expected: PASS (existing templateMatch host tests + the 4 new regionFeatures tests).

- [ ] **Step 7: Format and lint**

Run: `rtk cargo fmt -p rollshot-vision` then `rtk cargo clippy -p rollshot-vision --all-targets -- -D warnings`
Expected: no fmt diffs; clippy clean.

- [ ] **Step 8: Commit**

```bash
rtk git add crates/rollshot-vision/src/rect.rs crates/rollshot-vision/src/host.rs
rtk git commit -m "feat(vision): regionFeatures prepare + canonical-key callback

PR2/SP2: prepare_region_features computes the single feature for the query's
clipped pixel rect outside QuickJS and caches it under a canonical
RegionFeaturesKey{rect: PixelRect}. The callback resolves query.region to the
same key using stored image dimensions, so Full and an equivalent rect collapse
to one entry; it only looks up + truncates (no image work). limit==0 ->
invalid_query, unprepared -> vision_index_unavailable, limit>max -> LimitExceeded.
PixelRect gains Hash.

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 3 (PR3): role-free QuickJS integration fixture

**Files:**
- Create: `crates/rollshot-vision/tests/fixtures/region_features_top_bar.js`
- Create: `crates/rollshot-vision/tests/region_features.rs`
- Modify: `docs/superpowers/handoffs/2026-06-22-rollshot-vision.md` (append SP2 PR3 handoff note)

**Interfaces:**
- Consumes: `rollshot_vision::{RealAutomationHost, VisualIndex}`; `rollshot_automation::{execute_to_proposal, validate_source, AutomationInput, CancellationFlag, ExecutionPolicy, ProposalContext, ProposedEditKind, Region, RegionFeaturesQuery, ValidationLimits}`; `rollshot_automation_rquickjs::QuickJsExecutor`; `rollshot_edit_proposal::{EditProposal, ProposalId, ProposedEdit, Provenance, ProvenanceSource}`; `rollshot_image_document::ImageRect`.
- Produces: an end-to-end test proving a single-source `regionFeatures` detector runs through `QuickJsExecutor` + a prepared `RealAutomationHost` and yields the expected candidate. (No new product code; PR2 must already pass.)

- [ ] **Step 1: Create the fixture detector**

Create `crates/rollshot-vision/tests/fixtures/region_features_top_bar.js`. Single source = `regionFeatures`; dynamic width via `input.imageWidth`; emits a candidate only when the top strip is low-edge (a flat bar, not content):

```js
function main(input) {
  const strip = {
    kind: "rect",
    bounds: { x: 0, y: 0, width: input.imageWidth, height: 12 },
  };
  const features = rollshot.regionFeatures({ region: strip, limit: 1 });
  return {
    candidates: features
      .filter((f) => f.edgeDensity < 0.15)
      .map((f) => ({
        kind: "addRedaction",
        bounds: f.bounds,
        confidence: 0.7,
        label: "top-bar-region",
      })),
  };
}
```

- [ ] **Step 2: Create the integration test with the flat-strip (positive) case**

Create `crates/rollshot-vision/tests/region_features.rs`:

```rust
use std::time::Duration;

use rollshot_automation::{
    execute_to_proposal, validate_source, AutomationInput, CancellationFlag, ExecutionPolicy,
    ProposalContext, ProposedEditKind, Region, RegionFeaturesQuery, ValidationLimits,
};
use rollshot_automation_rquickjs::QuickJsExecutor;
use rollshot_edit_proposal::{EditProposal, ProposalId, ProposedEdit, Provenance, ProvenanceSource};
use rollshot_image_document::ImageRect;
use rollshot_vision::{RealAutomationHost, VisualIndex};

const TOP_BAR_JS: &str = include_str!("fixtures/region_features_top_bar.js");

const STRIP_HEIGHT: u32 = 12;

/// Scene with a flat top strip (rows 0..12) and a noisy body below.
fn scene(size: u32, flat_top: bool) -> image::RgbaImage {
    image::RgbaImage::from_fn(size, size, |x, y| {
        if y < STRIP_HEIGHT && flat_top {
            image::Rgba([200, 200, 200, 255])
        } else {
            let v = if (x + y) % 2 == 0 { 0 } else { 255 };
            image::Rgba([v, v, v, 255])
        }
    })
}

fn run(scene: image::RgbaImage) -> EditProposal {
    let (w, h) = scene.dimensions();
    let automation = validate_source(TOP_BAR_JS, &ValidationLimits::default()).unwrap();
    let input = AutomationInput {
        image_width: w,
        image_height: h,
        region: Some(Region::Full),
        annotations: Vec::new(),
        capability_handles: std::collections::BTreeMap::new(),
    };
    let proposal_ctx = ProposalContext {
        proposal_id: ProposalId(1),
        base_document_state_id: 0,
        provenance: Provenance { source: ProvenanceSource::Agent { run_id: 1 } },
    };
    let mut policy =
        ExecutionPolicy::smart_redaction_default(Duration::from_secs(2), 16 * 1024 * 1024, 256 * 1024);
    policy.allowed_edit_kinds.insert(ProposedEditKind::AddRedaction);

    let index = VisualIndex::build(scene).unwrap();
    // Prepare the SAME canonical rect the detector will query (dynamic width).
    let query = RegionFeaturesQuery {
        region: Region::Rect {
            bounds: ImageRect { x: 0.0, y: 0.0, width: w as f32, height: STRIP_HEIGHT as f32 },
        },
        limit: 1,
    };
    let mut host = RealAutomationHost::new();
    host.prepare_region_features(&index, &query).unwrap();

    let (proposal, _metrics) = execute_to_proposal(
        &QuickJsExecutor,
        &automation,
        &input,
        &proposal_ctx,
        &mut host,
        &policy,
        &CancellationFlag::new(),
    )
    .unwrap();
    proposal
}

#[test]
fn flat_top_strip_produces_one_candidate() {
    let proposal = run(scene(60, true));
    assert_eq!(proposal.candidates.len(), 1);
    match &proposal.candidates[0].edit {
        ProposedEdit::AddRedaction { bounds } => {
            assert_eq!(*bounds, ImageRect { x: 0.0, y: 0.0, width: 60.0, height: STRIP_HEIGHT as f32 });
        }
        other => panic!("expected AddRedaction, got {other:?}"),
    }
    assert_eq!(proposal.candidates[0].label, "top-bar-region");
}
```

- [ ] **Step 3: Run the positive test to verify it passes**

Run: `rtk cargo test -p rollshot-vision --test region_features flat_top_strip`
Expected: PASS — proves the regionFeatures path runs end-to-end through QuickJS (fixture validates, `input.imageWidth` resolves, prepared canonical rect is found).

> If `validate_source` rejects the fixture or the candidate count is wrong, that
> is a real defect surfaced here — debug it (systematic-debugging) before moving on;
> do not weaken the assertion.

- [ ] **Step 4: Add the negative and determinism cases**

Append to `crates/rollshot-vision/tests/region_features.rs`:

```rust
#[test]
fn noisy_top_strip_produces_no_candidates() {
    // No flat strip -> high edge density -> filter drops it.
    let proposal = run(scene(60, false));
    assert_eq!(proposal.candidates.len(), 0);
}

#[test]
fn region_features_detection_is_deterministic() {
    let a = run(scene(60, true));
    let b = run(scene(60, true));
    assert_eq!(a.candidates, b.candidates);
}
```

- [ ] **Step 5: Run the full integration test file**

Run: `rtk cargo test -p rollshot-vision --test region_features`
Expected: PASS (3 tests).

- [ ] **Step 6: Run the whole crate test suite + fmt + clippy**

Run: `rtk cargo test -p rollshot-vision`
Expected: PASS (all SP1 + SP2 unit and integration tests).
Run: `rtk cargo fmt --check -p rollshot-vision` then `rtk cargo clippy -p rollshot-vision --all-targets -- -D warnings`
Expected: clean.

- [ ] **Step 7: Append the SP2 PR3 handoff note**

Append to `docs/superpowers/handoffs/2026-06-22-rollshot-vision.md`:

```markdown

SP2 PR1 done — `region_features.rs`: deterministic `dominant_rgba` (quantized RGB histogram, bin-center output, lowest-bin tie-break) and `edge_density` (`|dx|+|dy|` over `(w-1)*(h-1)`, u64 accumulators, 0.0 for sub-2px rects). Pure functions, no host wiring. Next: PR2 prepare + callback.

SP2 PR2 done — `prepare_region_features` computes the single clipped-rect feature outside QuickJS and caches it under a canonical `RegionFeaturesKey{rect: PixelRect}`; the callback canonicalizes `query.region` via stored image dimensions and only looks up + truncates. Errors: `invalid_query` / `vision_index_unavailable` / `region_*` / `LimitExceeded`. `PixelRect` gained `Hash`. Next: PR3 integration.

SP2 PR3 done — SP2 complete. A role-free single-source `regionFeatures` detector (dynamic `input.imageWidth` top strip; the harness prepares the matching canonical rect) runs through `QuickJsExecutor` + prepared `RealAutomationHost`: flat strip → one candidate with clipped measured bounds, noisy strip → none, deterministic across runs. Deferred to later sub-projects: subregion splitting / RegionFeaturesV2 fields, manifest-gated full-image edge map, author inspectLayout (SP4), OCR (SP5), product/query-plan wiring (SP6).
```

- [ ] **Step 8: Commit**

```bash
rtk git add crates/rollshot-vision/tests/region_features.rs crates/rollshot-vision/tests/fixtures/region_features_top_bar.js docs/superpowers/handoffs/2026-06-22-rollshot-vision.md
rtk git commit -m "test(vision): role-free regionFeatures QuickJS integration (SP2 done)

PR3/SP2: a single-source regionFeatures detector (dynamic input.imageWidth top
strip) runs through QuickJsExecutor + a prepared RealAutomationHost. Flat strip
-> one candidate with clipped measured bounds; noisy strip -> none; deterministic
across runs. The harness prepares the matching canonical rect before execution,
exercising the canonical-key contract end-to-end. SP2 complete.

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Self-Review

**1. Spec coverage** (each spec section → task):
- §1 In scope: `regionFeatures` single-feature-per-region (Task 2 callback, Task 1 funcs); per-region prepare (Task 2); `region_features.rs` pure funcs (Task 1); error model (Task 2); role-free QuickJS fixture (Task 3). ✓
- §1 Not-in-scope (no splitting / no V2 fields / no edge map / no API change): enforced by Global Constraints + nothing in any task adds them. ✓
- §2 Existing boundaries used unchanged: Task 2 consumes them; no `rollshot-automation` edits. ✓
- §3.1 module layout + key type + PixelRect Hash: Task 1 (file), Task 2 (key + Hash). ✓
- §3.2 data flow + dynamic-query hard limit: Task 2 (prepare/callback), Task 3 (harness prepares canonical rect; no JS query inference). ✓
- §4 algorithms (QUANTIZE_STEP divides 256, bin center, tie-break; edge denominator/u64/sub-2px; alpha 255): Task 1 impl + tests. ✓
- §5 limit semantics + error codes: Task 2 (zero/limit/unprepared tests). ✓
- §6 privacy (aggregate only, no persistence): no serialization/persistence path added; nothing to implement. ✓
- §7 verification (unit + integration + commands): Task 1/2 unit, Task 3 integration, fmt/clippy steps. ✓
- §8 PR breakdown (PR1–PR3, handoff per PR): Tasks 1–3, handoff appended in Task 3 Step 7. ✓

**2. Placeholder scan:** No "TBD"/"add error handling"/"similar to Task N"; the only `todo!()` are deliberate Step-1 stubs that Steps 4/8 replace, and every code step shows full code. ✓

**3. Type consistency:** `dominant_rgba(&RgbaImage, PixelRect) -> [u8;4]` and `edge_density(&GrayImage, PixelRect) -> f32` are defined in Task 1 and called identically in Task 2; `RegionFeaturesKey{rect}`, `prepare_region_features(&mut self, &VisualIndex, &RegionFeaturesQuery)`, and the `region_features` callback signature match the trait in `rollshot-automation/src/host.rs`; `MAX_REGION_FEATURES_AREA`/`QUANTIZE_STEP`/`EDGE_THRESHOLD` names are consistent across tasks; `ImageRect`/`RegionFeaturesQuery`/`Region` usages match `rollshot-automation` and the SP1 integration test. ✓
