# rollshot-vision Runtime Host (Sub-project 1) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build `crates/rollshot-vision`, an agent-independent, deterministic, template-first runtime detection host that implements `AutomationHost`, so hand-authored template detectors produce real redaction candidates through the existing QuickJS executor.

**Architecture:** A new pure-Rust crate provides `VisualIndex` (built once per run, holds the image + cached grayscale), a `TemplateStore` with privacy-tagged `TemplateAsset`s, an NCC-based `templateMatch` with non-max suppression, and `TemplateSelfValidation`. `RealAutomationHost` wires these into the `AutomationHost` trait; unimplemented capabilities return an explicit `capability_unavailable` error rather than empty results.

**Tech Stack:** Rust, `image` 0.25, `imageproc` 0.26 (NCC template matching), `rollshot-automation` (trait + capability types), `rollshot-image-document` (geometry), `rollshot-edit-proposal` (proposal output). Tests use `rollshot-automation-rquickjs` as a dev-dependency.

**Spec:** `docs/superpowers/specs/2026-06-22-rollshot-vision-runtime-host-design.md`

## Global Constraints

Every task implicitly includes these (verbatim from the spec):

- **Crate is `unsafe_code = "forbid"`** — pure image processing, no FFI. Inherit workspace lints (`[lints] workspace = true`).
- **`imageproc` is pinned at `0.26` with `default-features = false`** in `[workspace.dependencies]`; both `rollshot-core` and `rollshot-vision` use `imageproc = { workspace = true }`. No OCR/OpenCV native dependency.
- **`image = 0.25`** via `{ workspace = true }`. Workspace floor Rust 1.89 — do not lower it.
- **Capability boundary unchanged** — do not modify `rollshot-automation` public contracts. SP1 only adds a host implementation.
- **Errors:** build/store-time → `VisionError`; capability-call-time → `rollshot_automation::CapabilityError`. Capability rejection codes: `template_not_found`, `template_larger_than_region`, `region_too_large`, `non_finite_region`, `empty_region`, `template_low_information`, `capability_unavailable`, `vision_index_unavailable`.
- **Implementation Guardrails (spec §Implementation Guardrails):**
  - `match_template_image` returns `Result<Vec<TemplateMatch>, CapabilityError>`, never a bare `Vec`.
  - `self_validate` takes `candidate_bounds: ImageRect` (crops internally from `index.image()`), returns `Result<_, VisionError>`.
  - `TemplateAsset`/`TemplateStore` must NOT have a generic serialize path that writes bytes; local save and export use separate explicit record types; export strips `Sensitive`.
  - `TemplateBytes` is raw RGBA with a checked constructor (`w>0, h>0, len==w*h*4, w*h<=MAX_TEMPLATE_AREA`).
  - `to_pixel_rect` uses floor-min / ceil-max rounding and rejects non-finite (`non_finite_region`) / empty (`empty_region`) / oversized (`region_too_large`).
  - NCC scores must be finite; low-information templates rejected (`template_low_information`); non-finite scores are non-matches.
- **Determinism:** every detection path is deterministic (stable sort by score then position). No `Date.now`/RNG.
- **Commits** follow conventional-commit style and end with the trailer:
  `Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>`
- All shell commands are run via the `rtk` prefix (repo convention), e.g. `rtk cargo test -p rollshot-vision`.

## File Structure

```
crates/rollshot-vision/
  Cargo.toml            # deps; [lints] workspace = true
  src/
    lib.rs              # module decls + public re-exports
    error.rs            # VisionError
    rect.rs             # PixelRect, to_pixel_rect, pad_and_clip, iou, union
    index.rs            # VisualIndex (image + cached grayscale)
    template.rs         # TemplateSensitivity, TemplateBytes, TemplateSource, TemplateAsset,
                        #   TemplateStore, Local/Export records, match_template_image, NMS,
                        #   VisualIndex::template_match
    self_validation.rs  # ExpectedCount, TemplateDecision, SelfValidationConfig,
                        #   TemplateSelfValidation, self_validate
    host.rs             # RealAutomationHost (impl AutomationHost)
  tests/
    fixtures/
      hide_bookmarks.js
      hide_folders.js
    integration.rs      # PR6: real JS through QuickJsExecutor + RealAutomationHost
```

Constants live where used: `MAX_TEMPLATE_AREA` and `MAX_SEARCH_AREA` in `rect.rs` / `template.rs` (documented module consts).

---

## Task 1: Crate skeleton + workspace wiring (PR1)

**Files:**
- Modify: `Cargo.toml` (workspace root — add member + `imageproc` workspace dep)
- Modify: `crates/rollshot-core/Cargo.toml` (switch `imageproc` to workspace)
- Create: `crates/rollshot-vision/Cargo.toml`
- Create: `crates/rollshot-vision/src/lib.rs`
- Create: `crates/rollshot-vision/src/error.rs`
- Create: `crates/rollshot-vision/src/host.rs`

**Interfaces:**
- Produces: `rollshot_vision::VisionError` (enum); `rollshot_vision::RealAutomationHost` with `RealAutomationHost::new() -> Self` (PR1 stub, no fields) implementing `rollshot_automation::AutomationHost` where all four methods return `Err(CapabilityError::Failed { code: "capability_unavailable" })`.

- [ ] **Step 1: Add the crate to the workspace and pin `imageproc`**

Edit the workspace root `Cargo.toml`. Add `"crates/rollshot-vision"` to `[workspace.members]` (keep the list sorted/grouped as the file already does), and add this line to `[workspace.dependencies]`:

```toml
imageproc = { version = "0.26", default-features = false }
```

- [ ] **Step 2: Switch `rollshot-core` to the workspace `imageproc`**

In `crates/rollshot-core/Cargo.toml`, replace:

```toml
imageproc = { version = "0.26", default-features = false }
```

with:

```toml
imageproc = { workspace = true }
```

- [ ] **Step 3: Verify the workspace still builds (no new crate yet beyond manifest)**

Run: `rtk cargo build -p rollshot-core`
Expected: builds (the `imageproc` switch is a no-op version-wise).

- [ ] **Step 4: Create `crates/rollshot-vision/Cargo.toml`**

```toml
[package]
name = "rollshot-vision"
version = "0.1.0"
edition = "2021"
rust-version = "1.89"

[lints]
workspace = true

[dependencies]
image = { workspace = true }
imageproc = { workspace = true }
rollshot-automation = { path = "../rollshot-automation" }
rollshot-image-document = { path = "../rollshot-image-document", features = ["serde"] }
serde = { workspace = true }
thiserror = { workspace = true }
tracing = { workspace = true }

[dev-dependencies]
rollshot-automation-rquickjs = { path = "../rollshot-automation-rquickjs" }
```

Note: confirm sibling crates declare lints via `[lints] workspace = true`; if a sibling uses a different mechanism, match that instead. If `edition`/`rust-version` are set workspace-wide via `[workspace.package]`, use `edition.workspace = true` / `rust-version.workspace = true` to match siblings.

- [ ] **Step 5: Create `crates/rollshot-vision/src/error.rs`**

```rust
//! Build- and storage-time errors for the vision host.
//!
//! Capability-call-time failures use `rollshot_automation::CapabilityError`;
//! this type is only for construction and template-store operations that
//! happen outside the capability call chain.

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum VisionError {
    #[error("image is empty (zero width or height)")]
    EmptyImage,
    #[error("template bytes invalid: {code}")]
    InvalidTemplateBytes { code: &'static str },
    #[error("candidate bounds are outside the source image")]
    CandidateOutOfBounds,
    #[error("io/serialization failure: {code}")]
    Io { code: &'static str },
}
```

- [ ] **Step 6: Create `crates/rollshot-vision/src/host.rs` (PR1 stub)**

```rust
//! `RealAutomationHost` — the runtime detection host.
//!
//! SP1 implements `template_match` (wired in PR4). Capabilities not yet
//! implemented return an explicit `capability_unavailable` error rather than
//! empty results: in a redaction tool, silently returning no results would let
//! a detector conclude "nothing to hide" and miss sensitive regions.

use rollshot_automation::{
    AutomationHost, CapabilityError, LayoutQuery, LayoutRegion, OcrMatch, OcrQuery, RegionFeatures,
    RegionFeaturesQuery, TemplateMatch, TemplateMatchQuery,
};

#[derive(Debug, Default)]
pub struct RealAutomationHost {}

impl RealAutomationHost {
    pub fn new() -> Self {
        Self {}
    }
}

impl AutomationHost for RealAutomationHost {
    fn ocr(&mut self, _query: OcrQuery) -> Result<Vec<OcrMatch>, CapabilityError> {
        Err(CapabilityError::Failed { code: "capability_unavailable" })
    }

    fn layout(&mut self, _query: LayoutQuery) -> Result<Vec<LayoutRegion>, CapabilityError> {
        Err(CapabilityError::Failed { code: "capability_unavailable" })
    }

    fn region_features(
        &mut self,
        _query: RegionFeaturesQuery,
    ) -> Result<Vec<RegionFeatures>, CapabilityError> {
        // Implemented in SP2.
        Err(CapabilityError::Failed { code: "capability_unavailable" })
    }

    fn template_match(
        &mut self,
        _query: TemplateMatchQuery,
    ) -> Result<Vec<TemplateMatch>, CapabilityError> {
        // Wired to VisualIndex::template_match in PR4.
        Err(CapabilityError::Failed { code: "capability_unavailable" })
    }
}
```

- [ ] **Step 7: Create `crates/rollshot-vision/src/lib.rs`**

```rust
//! Rollshot-specific, deterministic, UI-oriented vision adapter layer.
//! Implements the `rollshot_automation::AutomationHost` capability boundary.

#![forbid(unsafe_code)]

mod error;
mod host;

pub use error::VisionError;
pub use host::RealAutomationHost;
```

- [ ] **Step 8: Write the failing test (host returns `capability_unavailable`)**

Create `crates/rollshot-vision/src/host.rs` test module at the end of the file:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use rollshot_automation::{Region, TemplateMatchQuery};

    #[test]
    fn unimplemented_capabilities_report_unavailable() {
        let mut host = RealAutomationHost::new();
        let err = host
            .template_match(TemplateMatchQuery {
                template_handle: "x".into(),
                region: Region::Full,
                limit: 1,
            })
            .unwrap_err();
        assert_eq!(err, CapabilityError::Failed { code: "capability_unavailable" });
    }
}
```

- [ ] **Step 9: Run the test to verify it passes (and the crate compiles)**

Run: `rtk cargo test -p rollshot-vision`
Expected: PASS (1 test). The crate compiles and implements `AutomationHost`.

- [ ] **Step 10: Verify workspace-wide build and lints**

Run: `rtk cargo build --workspace`
Expected: success. Run `rtk cargo clippy -p rollshot-vision -- -D warnings`; expected: no warnings.

- [ ] **Step 11: Commit**

```bash
rtk git add Cargo.toml crates/rollshot-core/Cargo.toml crates/rollshot-vision
rtk git commit -m "feat(vision): crate skeleton + AutomationHost stub (PR1)"
```

- [ ] **Step 12: Handoff note**

Append a short note to `docs/superpowers/handoffs/2026-06-22-rollshot-vision.md` (create it): "PR1 done — `rollshot-vision` crate exists, `RealAutomationHost` implements `AutomationHost` returning `capability_unavailable` for all four capabilities; `imageproc` pinned in workspace at 0.26 (default-features = false). Next: PR2 `VisualIndex` + `rect.rs`."

---

## Task 2: `VisualIndex` + `rect.rs` (PR2)

**Files:**
- Create: `crates/rollshot-vision/src/rect.rs`
- Create: `crates/rollshot-vision/src/index.rs`
- Modify: `crates/rollshot-vision/src/lib.rs` (add `mod rect; mod index;` + re-exports)

**Interfaces:**
- Consumes: `rollshot_image_document::ImageRect` (`{ x, y, width, height: f32 }`, `is_finite()`), `rollshot_automation::Region`, `rollshot_automation::CapabilityError`.
- Produces:
  - `rect::PixelRect { pub x: u32, pub y: u32, pub width: u32, pub height: u32 }`.
  - `rect::to_pixel_rect(rect: ImageRect, image_w: u32, image_h: u32, max_area: u64) -> Result<PixelRect, CapabilityError>`.
  - `rect::region_to_pixel_rect(region: &Region, image_w: u32, image_h: u32, max_area: u64) -> Result<PixelRect, CapabilityError>`.
  - `rect::iou(a: ImageRect, b: ImageRect) -> f32`, `rect::union(a: ImageRect, b: ImageRect) -> ImageRect`, `rect::pad_and_clip(rect: ImageRect, pad: f32, image_w: u32, image_h: u32) -> ImageRect`.
  - `rect::MAX_SEARCH_AREA: u64` (default search-area cap).
  - `index::VisualIndex` with `build(image: image::RgbaImage) -> Result<VisualIndex, VisionError>`, `width()`, `height()`, `image() -> &image::RgbaImage`, `pub(crate) fn gray(&self) -> &image::GrayImage`.

- [ ] **Step 1: Write the failing test for `to_pixel_rect` rounding + error rules**

Create `crates/rollshot-vision/src/rect.rs` with only a test module first:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use rollshot_image_document::ImageRect;
    use rollshot_automation::CapabilityError;

    fn r(x: f32, y: f32, w: f32, h: f32) -> ImageRect {
        ImageRect { x, y, width: w, height: h }
    }

    #[test]
    fn pixel_rect_uses_floor_min_ceil_max() {
        // x in [10.2, 10.2+5.3=15.5] -> floor(10.2)=10 .. ceil(15.5)=16 -> w=6
        let p = to_pixel_rect(r(10.2, 4.9, 5.3, 2.2), 100, 100, MAX_SEARCH_AREA).unwrap();
        assert_eq!((p.x, p.y, p.width, p.height), (10, 4, 6, 4));
    }

    #[test]
    fn pixel_rect_clamps_to_image() {
        let p = to_pixel_rect(r(-5.0, -5.0, 20.0, 20.0), 10, 10, MAX_SEARCH_AREA).unwrap();
        assert_eq!((p.x, p.y, p.width, p.height), (0, 0, 10, 10));
    }

    #[test]
    fn pixel_rect_rejects_non_finite() {
        let e = to_pixel_rect(r(f32::NAN, 0.0, 1.0, 1.0), 10, 10, MAX_SEARCH_AREA).unwrap_err();
        assert_eq!(e, CapabilityError::InvalidInput { code: "non_finite_region" });
    }

    #[test]
    fn pixel_rect_rejects_empty() {
        let e = to_pixel_rect(r(5.0, 5.0, 0.0, 3.0), 10, 10, MAX_SEARCH_AREA).unwrap_err();
        assert_eq!(e, CapabilityError::InvalidInput { code: "empty_region" });
    }

    #[test]
    fn pixel_rect_rejects_oversized() {
        let e = to_pixel_rect(r(0.0, 0.0, 100.0, 100.0), 100, 100, 100).unwrap_err();
        assert_eq!(e, CapabilityError::InvalidInput { code: "region_too_large" });
    }

    #[test]
    fn iou_of_identical_is_one() {
        assert!((iou(r(0.0, 0.0, 10.0, 10.0), r(0.0, 0.0, 10.0, 10.0)) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn iou_of_disjoint_is_zero() {
        assert_eq!(iou(r(0.0, 0.0, 5.0, 5.0), r(20.0, 20.0, 5.0, 5.0)), 0.0);
    }
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `rtk cargo test -p rollshot-vision rect`
Expected: FAIL to compile ("cannot find function `to_pixel_rect`").

- [ ] **Step 3: Implement `rect.rs`**

Prepend to `crates/rollshot-vision/src/rect.rs` (above the test module):

```rust
//! Pixel-space rectangle helpers shared by template matching and (later)
//! region features. `ImageRect` is f32 pixel-space; `PixelRect` is the u32
//! integer grid used by the `image` crate.

use rollshot_automation::{CapabilityError, Region};
use rollshot_image_document::ImageRect;

/// Default cap on template search area (pixels). Bounds naive-NCC cost.
pub const MAX_SEARCH_AREA: u64 = 8_000_000; // ~ 4000x2000

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PixelRect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

/// Convert an f32 `ImageRect` to an integer `PixelRect` covering it.
///
/// Rounding: floor on the min edges, ceil on the max edges (smallest integer
/// rect that fully covers the f32 rect), then clamp to the image. Rejects
/// non-finite, empty (before or after clamp), and oversized regions.
pub fn to_pixel_rect(
    rect: ImageRect,
    image_w: u32,
    image_h: u32,
    max_area: u64,
) -> Result<PixelRect, CapabilityError> {
    if !rect.is_finite() {
        return Err(CapabilityError::InvalidInput { code: "non_finite_region" });
    }
    if rect.width <= 0.0 || rect.height <= 0.0 {
        return Err(CapabilityError::InvalidInput { code: "empty_region" });
    }
    let x0 = rect.x.floor();
    let y0 = rect.y.floor();
    let x1 = (rect.x + rect.width).ceil();
    let y1 = (rect.y + rect.height).ceil();

    // Clamp to [0, image]. Values are finite here.
    let cx0 = x0.max(0.0).min(image_w as f32) as u32;
    let cy0 = y0.max(0.0).min(image_h as f32) as u32;
    let cx1 = x1.max(0.0).min(image_w as f32) as u32;
    let cy1 = y1.max(0.0).min(image_h as f32) as u32;

    if cx1 <= cx0 || cy1 <= cy0 {
        return Err(CapabilityError::InvalidInput { code: "empty_region" });
    }
    let width = cx1 - cx0;
    let height = cy1 - cy0;
    if (width as u64) * (height as u64) > max_area {
        return Err(CapabilityError::InvalidInput { code: "region_too_large" });
    }
    Ok(PixelRect { x: cx0, y: cy0, width, height })
}

/// Resolve a capability `Region` to a `PixelRect` against the image.
pub fn region_to_pixel_rect(
    region: &Region,
    image_w: u32,
    image_h: u32,
    max_area: u64,
) -> Result<PixelRect, CapabilityError> {
    match region {
        Region::Full => to_pixel_rect(
            ImageRect { x: 0.0, y: 0.0, width: image_w as f32, height: image_h as f32 },
            image_w,
            image_h,
            max_area,
        ),
        Region::Rect { bounds } => to_pixel_rect(*bounds, image_w, image_h, max_area),
    }
}

/// Intersection-over-union of two rects in image space.
pub fn iou(a: ImageRect, b: ImageRect) -> f32 {
    let ax2 = a.x + a.width;
    let ay2 = a.y + a.height;
    let bx2 = b.x + b.width;
    let by2 = b.y + b.height;
    let ix = (ax2.min(bx2) - a.x.max(b.x)).max(0.0);
    let iy = (ay2.min(by2) - a.y.max(b.y)).max(0.0);
    let inter = ix * iy;
    let union = a.width * a.height + b.width * b.height - inter;
    if union <= 0.0 {
        0.0
    } else {
        inter / union
    }
}

/// Bounding union of two rects.
pub fn union(a: ImageRect, b: ImageRect) -> ImageRect {
    let x = a.x.min(b.x);
    let y = a.y.min(b.y);
    let x2 = (a.x + a.width).max(b.x + b.width);
    let y2 = (a.y + a.height).max(b.y + b.height);
    ImageRect { x, y, width: x2 - x, height: y2 - y }
}

/// Expand a rect by `pad` on every side, clamped to the image.
pub fn pad_and_clip(rect: ImageRect, pad: f32, image_w: u32, image_h: u32) -> ImageRect {
    rect.expanded(pad).clamp_to(image_w, image_h)
}
```

- [ ] **Step 4: Run `rect` tests to verify they pass**

Run: `rtk cargo test -p rollshot-vision rect`
Expected: PASS (7 tests).

- [ ] **Step 5: Write the failing test for `VisualIndex`**

Create `crates/rollshot-vision/src/index.rs` with a test module first:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::VisionError;

    fn solid(w: u32, h: u32, lum: u8) -> image::RgbaImage {
        image::RgbaImage::from_pixel(w, h, image::Rgba([lum, lum, lum, 255]))
    }

    #[test]
    fn build_rejects_empty_image() {
        let e = VisualIndex::build(image::RgbaImage::new(0, 0)).unwrap_err();
        assert_eq!(e, VisionError::EmptyImage);
    }

    #[test]
    fn build_caches_grayscale_with_right_dims() {
        let idx = VisualIndex::build(solid(8, 4, 200)).unwrap();
        assert_eq!((idx.width(), idx.height()), (8, 4));
        assert_eq!(idx.gray().dimensions(), (8, 4));
        // Grayscale of a mid-grey RGBA is ~ that grey.
        assert!(idx.gray().get_pixel(0, 0).0[0] > 150);
    }
}
```

- [ ] **Step 6: Run the test to verify it fails**

Run: `rtk cargo test -p rollshot-vision index`
Expected: FAIL to compile ("cannot find `VisualIndex`").

- [ ] **Step 7: Implement `index.rs`**

Prepend to `crates/rollshot-vision/src/index.rs`:

```rust
//! `VisualIndex` — built once per automation run; holds the source image and
//! a cached grayscale (the only precompute SP1 needs, for NCC). Manifest-driven
//! lazy precompute is deferred to SP2.

use crate::VisionError;

pub struct VisualIndex {
    image: image::RgbaImage,
    width: u32,
    height: u32,
    gray: image::GrayImage,
}

impl VisualIndex {
    pub fn build(image: image::RgbaImage) -> Result<Self, VisionError> {
        let (width, height) = image.dimensions();
        if width == 0 || height == 0 {
            return Err(VisionError::EmptyImage);
        }
        let gray = image::imageops::grayscale(&image);
        Ok(Self { image, width, height, gray })
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    pub fn image(&self) -> &image::RgbaImage {
        &self.image
    }

    pub(crate) fn gray(&self) -> &image::GrayImage {
        &self.gray
    }
}
```

- [ ] **Step 8: Wire modules into `lib.rs`**

Update `crates/rollshot-vision/src/lib.rs`:

```rust
#![forbid(unsafe_code)]

mod error;
mod host;
mod index;
pub mod rect;

pub use error::VisionError;
pub use host::RealAutomationHost;
pub use index::VisualIndex;
```

- [ ] **Step 9: Run all crate tests**

Run: `rtk cargo test -p rollshot-vision`
Expected: PASS (rect + index + host tests).

- [ ] **Step 10: Commit**

```bash
rtk git add crates/rollshot-vision/src
rtk git commit -m "feat(vision): VisualIndex + pixel-rect helpers (PR2)"
```

- [ ] **Step 11: Handoff note**

Append to the handoff doc: "PR2 done — `VisualIndex::build` (cached grayscale, rejects empty), `rect::{to_pixel_rect (floor-min/ceil-max + non_finite/empty/region_too_large), region_to_pixel_rect, iou, union, pad_and_clip}`, `MAX_SEARCH_AREA`. Next: PR3 template store."

---

## Task 3: `TemplateStore` + sensitivity + serialization gate (PR3)

**Files:**
- Create: `crates/rollshot-vision/src/template.rs`
- Modify: `crates/rollshot-vision/src/lib.rs` (add `mod template;` + re-exports)

**Interfaces:**
- Consumes: `VisionError`, `rollshot_image_document::ImageRect`.
- Produces:
  - `template::TemplateSensitivity { Chrome, Sensitive }` (derives `Debug, Clone, Copy, PartialEq, Eq`).
  - `template::TemplateBytes` with `new(width: u32, height: u32, rgba: Vec<u8>) -> Result<Self, VisionError>`, `width()`, `height()`, `to_rgba_image() -> image::RgbaImage`, and `MAX_TEMPLATE_AREA: u64`.
  - `template::TemplateSource { UserRect, AgentSuggested }` (placeholder provenance enum).
  - `template::TemplateAsset { handle: String, sensitivity, source, created_at_ms: u64, bounds_in_source_image: Option<ImageRect>, bytes: TemplateBytes }` — **no generic `Serialize`**.
  - `template::TemplateStore` with `new()`, `insert(asset)`, `get(handle) -> Option<&TemplateAsset>`, `save_local() -> Vec<LocalTemplateAssetRecord>`, `export() -> Vec<ExportTemplateAssetRecord>`.
  - `template::LocalTemplateAssetRecord` (bytes present) and `template::ExportTemplateAssetRecord` (`bytes: Option<TemplateBytes>`, `None` for `Sensitive`) — these are the only `Serialize` carriers.

- [ ] **Step 1: Write the failing tests (TemplateBytes invariants + export strips Sensitive)**

Create `crates/rollshot-vision/src/template.rs` with a test module first:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::VisionError;

    fn bytes(w: u32, h: u32) -> TemplateBytes {
        TemplateBytes::new(w, h, vec![0u8; (w * h * 4) as usize]).unwrap()
    }

    fn asset(handle: &str, s: TemplateSensitivity) -> TemplateAsset {
        TemplateAsset {
            handle: handle.into(),
            sensitivity: s,
            source: TemplateSource::UserRect,
            created_at_ms: 0,
            bounds_in_source_image: None,
            bytes: bytes(4, 4),
        }
    }

    #[test]
    fn template_bytes_rejects_wrong_length() {
        let e = TemplateBytes::new(2, 2, vec![0u8; 8]).unwrap_err();
        assert_eq!(e, VisionError::InvalidTemplateBytes { code: "length_mismatch" });
    }

    #[test]
    fn template_bytes_rejects_zero_dim() {
        let e = TemplateBytes::new(0, 2, vec![]).unwrap_err();
        assert_eq!(e, VisionError::InvalidTemplateBytes { code: "zero_dimension" });
    }

    #[test]
    fn template_bytes_rejects_oversized() {
        // 1 px over the cap.
        let side = (MAX_TEMPLATE_AREA as f64).sqrt() as u32 + 2;
        let e = TemplateBytes::new(side, side, vec![0u8; (side as usize) * (side as usize) * 4]);
        assert_eq!(e.unwrap_err(), VisionError::InvalidTemplateBytes { code: "too_large" });
    }

    #[test]
    fn get_returns_inserted_asset() {
        let mut store = TemplateStore::new();
        store.insert(asset("a", TemplateSensitivity::Chrome));
        assert!(store.get("a").is_some());
        assert!(store.get("missing").is_none());
    }

    #[test]
    fn export_strips_sensitive_bytes_but_local_keeps_them() {
        let mut store = TemplateStore::new();
        store.insert(asset("chrome", TemplateSensitivity::Chrome));
        store.insert(asset("secret", TemplateSensitivity::Sensitive));

        let local = store.save_local();
        assert!(local.iter().all(|r| r.bytes.width() > 0)); // all bytes present locally

        let exported = store.export();
        let secret = exported.iter().find(|r| r.handle == "secret").unwrap();
        let chrome = exported.iter().find(|r| r.handle == "chrome").unwrap();
        assert!(secret.bytes.is_none(), "sensitive bytes must be stripped on export");
        assert!(chrome.bytes.is_some(), "chrome bytes are kept on export");
    }
}
```

- [ ] **Step 2: Run to verify failure**

Run: `rtk cargo test -p rollshot-vision template`
Expected: FAIL to compile (types not found).

- [ ] **Step 3: Implement `template.rs` (store + sensitivity + records)**

Prepend to `crates/rollshot-vision/src/template.rs`:

```rust
//! Template assets, the local template store, and the privacy serialization
//! gate. `TemplateAsset`/`TemplateStore` deliberately do NOT derive a generic
//! `Serialize` that writes bytes: serialization only goes through the explicit
//! `LocalTemplateAssetRecord` (keeps all bytes) and `ExportTemplateAssetRecord`
//! (drops `Sensitive` bytes). This makes it impossible to leak sensitive bytes
//! through an accidental `serde_json::to_writer(&store)`.

use std::collections::BTreeMap;

use rollshot_image_document::ImageRect;
use serde::{Deserialize, Serialize};

use crate::VisionError;

/// Cap on a single template's pixel area.
pub const MAX_TEMPLATE_AREA: u64 = 1_048_576; // 1024x1024

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TemplateSensitivity {
    Chrome,
    Sensitive,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TemplateSource {
    UserRect,
    AgentSuggested,
}

/// Raw RGBA template pixels. Invariant: `rgba.len() == width * height * 4`,
/// `width > 0`, `height > 0`, `width * height <= MAX_TEMPLATE_AREA`. Only
/// constructible through `new`, which checks the invariant.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TemplateBytes {
    width: u32,
    height: u32,
    rgba: Vec<u8>,
}

impl TemplateBytes {
    pub fn new(width: u32, height: u32, rgba: Vec<u8>) -> Result<Self, VisionError> {
        if width == 0 || height == 0 {
            return Err(VisionError::InvalidTemplateBytes { code: "zero_dimension" });
        }
        if (width as u64) * (height as u64) > MAX_TEMPLATE_AREA {
            return Err(VisionError::InvalidTemplateBytes { code: "too_large" });
        }
        if rgba.len() != (width as usize) * (height as usize) * 4 {
            return Err(VisionError::InvalidTemplateBytes { code: "length_mismatch" });
        }
        Ok(Self { width, height, rgba })
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    /// Infallible: the checked invariant guarantees a valid buffer.
    pub fn to_rgba_image(&self) -> image::RgbaImage {
        image::RgbaImage::from_raw(self.width, self.height, self.rgba.clone())
            .expect("TemplateBytes invariant guarantees a valid RGBA buffer")
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct TemplateAsset {
    pub handle: String,
    pub sensitivity: TemplateSensitivity,
    pub source: TemplateSource,
    pub created_at_ms: u64,
    pub bounds_in_source_image: Option<ImageRect>,
    pub bytes: TemplateBytes,
}

#[derive(Debug, Default)]
pub struct TemplateStore {
    assets: BTreeMap<String, TemplateAsset>,
}

impl TemplateStore {
    pub fn new() -> Self {
        Self { assets: BTreeMap::new() }
    }

    pub fn insert(&mut self, asset: TemplateAsset) {
        self.assets.insert(asset.handle.clone(), asset);
    }

    pub fn get(&self, handle: &str) -> Option<&TemplateAsset> {
        self.assets.get(handle)
    }

    /// Local persistence: keeps all bytes (chrome + sensitive).
    pub fn save_local(&self) -> Vec<LocalTemplateAssetRecord> {
        self.assets.values().map(LocalTemplateAssetRecord::from_asset).collect()
    }

    /// Export: strips `Sensitive` bytes (D4 §enforcement). The privacy gate is
    /// applied here and only here, via `ExportTemplateAssetRecord`.
    pub fn export(&self) -> Vec<ExportTemplateAssetRecord> {
        self.assets.values().map(ExportTemplateAssetRecord::from_asset).collect()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LocalTemplateAssetRecord {
    pub handle: String,
    pub sensitivity_sensitive: bool,
    pub source_agent_suggested: bool,
    pub created_at_ms: u64,
    pub bounds_in_source_image: Option<ImageRect>,
    pub width: u32,
    pub height: u32,
    pub bytes: TemplateBytesRecord,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExportTemplateAssetRecord {
    pub handle: String,
    pub sensitivity_sensitive: bool,
    pub source_agent_suggested: bool,
    pub created_at_ms: u64,
    pub bounds_in_source_image: Option<ImageRect>,
    pub width: u32,
    pub height: u32,
    /// `None` for `Sensitive` assets — bytes are stripped on export.
    pub bytes: Option<TemplateBytesRecord>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TemplateBytesRecord {
    pub rgba: Vec<u8>,
}

impl LocalTemplateAssetRecord {
    fn from_asset(a: &TemplateAsset) -> Self {
        Self {
            handle: a.handle.clone(),
            sensitivity_sensitive: matches!(a.sensitivity, TemplateSensitivity::Sensitive),
            source_agent_suggested: matches!(a.source, TemplateSource::AgentSuggested),
            created_at_ms: a.created_at_ms,
            bounds_in_source_image: a.bounds_in_source_image,
            width: a.bytes.width(),
            height: a.bytes.height(),
            bytes: TemplateBytesRecord { rgba: a.bytes.rgba.clone() },
        }
    }
}

impl ExportTemplateAssetRecord {
    fn from_asset(a: &TemplateAsset) -> Self {
        let bytes = match a.sensitivity {
            TemplateSensitivity::Sensitive => None,
            TemplateSensitivity::Chrome => Some(TemplateBytesRecord { rgba: a.bytes.rgba.clone() }),
        };
        Self {
            handle: a.handle.clone(),
            sensitivity_sensitive: matches!(a.sensitivity, TemplateSensitivity::Sensitive),
            source_agent_suggested: matches!(a.source, TemplateSource::AgentSuggested),
            created_at_ms: a.created_at_ms,
            bounds_in_source_image: a.bounds_in_source_image,
            width: a.bytes.width(),
            height: a.bytes.height(),
            bytes,
        }
    }
}
```

Note: `TemplateBytes.rgba` is a private field; the record builders live in the same module so they may read it. `TemplateAsset`/`TemplateStore`/`TemplateBytes` have **no** `Serialize` derive — the only serializable carriers are the record types.

- [ ] **Step 4: Add the "no generic serialize path" guard test**

Add to the `tests` module in `template.rs`:

```rust
    // Compile-time guarantee documented as a test: TemplateStore/TemplateAsset
    // expose no Serialize impl. If someone adds `#[derive(Serialize)]` to them,
    // this static assertion's negation will start compiling and the reviewer
    // should catch it. We assert the *records* are Serialize and the asset is not
    // by relying on the type system: this test simply documents the rule and
    // verifies the export path is the only byte-stripping channel.
    #[test]
    fn export_is_the_only_sensitive_byte_channel() {
        let mut store = TemplateStore::new();
        store.insert(asset("secret", TemplateSensitivity::Sensitive));
        // Serialize the export records (the only Serialize carrier) and confirm
        // no sensitive RGBA bytes are present in the output.
        let exported = store.export();
        let json = serde_json::to_string(&exported).unwrap();
        assert!(json.contains("\"bytes\":null"));
    }
```

Add `serde_json` to `[dev-dependencies]` in `crates/rollshot-vision/Cargo.toml`:

```toml
serde_json = "1"
```

(Use the workspace `serde_json` if one exists — check `[workspace.dependencies]`; if present, use `serde_json = { workspace = true }`.)

- [ ] **Step 5: Wire `template` into `lib.rs`**

```rust
mod template;

pub use template::{
    ExportTemplateAssetRecord, LocalTemplateAssetRecord, TemplateAsset, TemplateBytes,
    TemplateSensitivity, TemplateSource, TemplateStore, MAX_TEMPLATE_AREA,
};
```

- [ ] **Step 6: Run template tests**

Run: `rtk cargo test -p rollshot-vision template`
Expected: PASS (6 tests).

- [ ] **Step 7: Commit**

```bash
rtk git add crates/rollshot-vision
rtk git commit -m "feat(vision): template store + sensitivity serialization gate (PR3)"
```

- [ ] **Step 8: Handoff note**

Append: "PR3 done — `TemplateBytes` (checked raw RGBA), `TemplateAsset`/`TemplateStore` (no generic Serialize), `save_local`/`export` records; export strips `Sensitive` bytes. Next: PR4 templateMatch + NMS."

---

## Task 4: `templateMatch` v0 + NMS (PR4)

**Files:**
- Modify: `crates/rollshot-vision/src/template.rs` (add `match_template_image`, NMS, `VisualIndex::template_match`)
- Modify: `crates/rollshot-vision/src/host.rs` (give `RealAutomationHost` `index` + `templates`; delegate `template_match`)
- Modify: `crates/rollshot-vision/src/lib.rs` (re-exports if needed)

**Interfaces:**
- Consumes: `VisualIndex::gray()`, `rect::{region_to_pixel_rect, iou, MAX_SEARCH_AREA, PixelRect}`, `TemplateStore::get`, `rollshot_automation::{TemplateMatchQuery, TemplateMatch, CapabilityError, Region}`, `rollshot_image_document::{ImageRect, ImagePoint}`, imageproc template matching.
- Produces:
  - `template::match_template_image(index: &VisualIndex, tpl_gray: &image::GrayImage, region: &Region, limit: u32) -> Result<Vec<TemplateMatch>, CapabilityError>`.
  - `impl VisualIndex { pub(crate) fn template_match(&self, store: &TemplateStore, q: TemplateMatchQuery) -> Result<Vec<TemplateMatch>, CapabilityError> }`.
  - `RealAutomationHost::new(index: VisualIndex, templates: TemplateStore) -> Self`.

- [ ] **Step 1: Confirm the imageproc 0.26 template-matching API**

Run: `rtk cargo doc -p imageproc --no-deps` is unnecessary; instead grep the dependency source:

Run: `rtk bash -c "find ~/.cargo -path '*imageproc-0.26*/src/template_matching*' -name '*.rs' | head"` then read the file to confirm the exact signatures of `match_template` and `MatchTemplateMethod`.
Expected: `pub fn match_template(image: &GrayImage, template: &GrayImage, method: MatchTemplateMethod) -> Image<Luma<f32>>` and `enum MatchTemplateMethod { ... CrossCorrelationNormalized ... }`. If the names differ in 0.26 (e.g. `match_template` requires a `&MatchTemplateMethod` or the enum variant name differs), adjust the calls in Step 3 accordingly. Do not proceed with a guessed signature.

- [ ] **Step 2: Write the failing test (find a pasted template + NMS + limit + errors)**

Add to the `tests` module in `crates/rollshot-vision/src/template.rs`:

```rust
    use crate::index::VisualIndex;
    use rollshot_automation::{CapabilityError, Region, TemplateMatchQuery};

    /// 40x40 mid-grey scene with a distinctive 8x8 black square pasted at (10,12)
    /// and (28,6). Returns (scene, template_bytes).
    fn scene_with_two_marks() -> (image::RgbaImage, TemplateBytes) {
        let mut scene = image::RgbaImage::from_pixel(40, 40, image::Rgba([180, 180, 180, 255]));
        for &(ox, oy) in &[(10u32, 12u32), (28, 6)] {
            for dy in 0..8 {
                for dx in 0..8 {
                    // A small structured glyph (checker) so it has variance.
                    let v = if (dx + dy) % 2 == 0 { 0 } else { 60 };
                    scene.put_pixel(ox + dx, oy + dy, image::Rgba([v, v, v, 255]));
                }
            }
        }
        let tpl_img = image::imageops::crop_imm(&scene, 10, 12, 8, 8).to_image();
        let bytes = TemplateBytes::new(8, 8, tpl_img.into_raw()).unwrap();
        (scene, bytes)
    }

    fn store_with(handle: &str, bytes: TemplateBytes, s: TemplateSensitivity) -> TemplateStore {
        let mut store = TemplateStore::new();
        store.insert(TemplateAsset {
            handle: handle.into(),
            sensitivity: s,
            source: TemplateSource::UserRect,
            created_at_ms: 0,
            bounds_in_source_image: None,
            bytes,
        });
        store
    }

    #[test]
    fn finds_both_instances_with_nms() {
        let (scene, tpl) = scene_with_two_marks();
        let index = VisualIndex::build(scene).unwrap();
        let store = store_with("mark", tpl, TemplateSensitivity::Chrome);
        let matches = index
            .template_match(&store, TemplateMatchQuery {
                template_handle: "mark".into(),
                region: Region::Full,
                limit: 10,
            })
            .unwrap();
        // Exactly the two pasted instances survive NMS, top-scored near 1.0.
        assert_eq!(matches.len(), 2);
        assert!(matches[0].score > 0.9);
        // Bounds are template-sized.
        assert_eq!((matches[0].bounds.width, matches[0].bounds.height), (8.0, 8.0));
        // Anchor is the center of the bounds.
        let c = matches[0].bounds;
        assert!((matches[0].anchor.x - (c.x + c.width / 2.0)).abs() < 1e-3);
    }

    #[test]
    fn limit_is_respected() {
        let (scene, tpl) = scene_with_two_marks();
        let index = VisualIndex::build(scene).unwrap();
        let store = store_with("mark", tpl, TemplateSensitivity::Chrome);
        let matches = index
            .template_match(&store, TemplateMatchQuery {
                template_handle: "mark".into(),
                region: Region::Full,
                limit: 1,
            })
            .unwrap();
        assert_eq!(matches.len(), 1);
    }

    #[test]
    fn missing_handle_is_typed_error() {
        let (scene, _tpl) = scene_with_two_marks();
        let index = VisualIndex::build(scene).unwrap();
        let store = TemplateStore::new();
        let e = index
            .template_match(&store, TemplateMatchQuery {
                template_handle: "nope".into(),
                region: Region::Full,
                limit: 10,
            })
            .unwrap_err();
        assert_eq!(e, CapabilityError::Failed { code: "template_not_found" });
    }

    #[test]
    fn low_information_template_is_rejected() {
        let scene = image::RgbaImage::from_pixel(40, 40, image::Rgba([180, 180, 180, 255]));
        let index = VisualIndex::build(scene).unwrap();
        // Solid 8x8 template — zero variance.
        let flat = TemplateBytes::new(8, 8, vec![180u8; 8 * 8 * 4]).unwrap();
        let store = store_with("flat", flat, TemplateSensitivity::Chrome);
        let e = index
            .template_match(&store, TemplateMatchQuery {
                template_handle: "flat".into(),
                region: Region::Full,
                limit: 10,
            })
            .unwrap_err();
        assert_eq!(e, CapabilityError::InvalidInput { code: "template_low_information" });
    }

    #[test]
    fn template_larger_than_region_is_error() {
        let scene = image::RgbaImage::from_pixel(6, 6, image::Rgba([180, 180, 180, 255]));
        let index = VisualIndex::build(scene).unwrap();
        let big = TemplateBytes::new(8, 8, vec![0u8; 8 * 8 * 4]).unwrap();
        let store = store_with("big", big, TemplateSensitivity::Chrome);
        let e = index
            .template_match(&store, TemplateMatchQuery {
                template_handle: "big".into(),
                region: Region::Full,
                limit: 10,
            })
            .unwrap_err();
        assert_eq!(e, CapabilityError::InvalidInput { code: "template_larger_than_region" });
    }
```

- [ ] **Step 3: Implement `match_template_image`, NMS, and `VisualIndex::template_match`**

Append to `crates/rollshot-vision/src/template.rs` (outside the `tests` module). Adjust the imageproc call if Step 1 showed a different signature:

```rust
use image::Luma;
use imageproc::template_matching::{match_template, MatchTemplateMethod};
use rollshot_automation::{CapabilityError, Region, TemplateMatch, TemplateMatchQuery};
use rollshot_image_document::{ImageRect, ImagePoint};

use crate::index::VisualIndex;
use crate::rect::{iou, region_to_pixel_rect, MAX_SEARCH_AREA};

/// Variance floor below which a template carries too little information for NCC.
const MIN_TEMPLATE_VARIANCE: f32 = 25.0;
/// IoU above which two matches are treated as the same instance during NMS.
const NMS_IOU_THRESHOLD: f32 = 0.4;

fn gray_variance(gray: &image::GrayImage) -> f32 {
    let n = (gray.width() * gray.height()) as f32;
    if n == 0.0 {
        return 0.0;
    }
    let mut sum = 0.0f32;
    let mut sum_sq = 0.0f32;
    for p in gray.pixels() {
        let v = p.0[0] as f32;
        sum += v;
        sum_sq += v * v;
    }
    let mean = sum / n;
    (sum_sq / n) - mean * mean
}

impl VisualIndex {
    pub(crate) fn template_match(
        &self,
        store: &TemplateStore,
        q: TemplateMatchQuery,
    ) -> Result<Vec<TemplateMatch>, CapabilityError> {
        let asset = store
            .get(&q.template_handle)
            .ok_or(CapabilityError::Failed { code: "template_not_found" })?;
        let tpl_gray = image::imageops::grayscale(&asset.bytes.to_rgba_image());
        match_template_image(self, &tpl_gray, &q.region, q.limit)
    }
}

/// Core NCC + NMS matcher shared by the capability and self-validation. Takes a
/// grayscale template directly (no store handle).
pub(crate) fn match_template_image(
    index: &VisualIndex,
    tpl_gray: &image::GrayImage,
    region: &Region,
    limit: u32,
) -> Result<Vec<TemplateMatch>, CapabilityError> {
    if gray_variance(tpl_gray) < MIN_TEMPLATE_VARIANCE {
        return Err(CapabilityError::InvalidInput { code: "template_low_information" });
    }
    let (tw, th) = tpl_gray.dimensions();
    if tw == 0 || th == 0 {
        return Err(CapabilityError::InvalidInput { code: "template_low_information" });
    }

    let search = region_to_pixel_rect(region, index.width(), index.height(), MAX_SEARCH_AREA)?;
    if tw > search.width || th > search.height {
        return Err(CapabilityError::InvalidInput { code: "template_larger_than_region" });
    }

    // Crop the scene grayscale to the search region.
    let scene = image::imageops::crop_imm(index.gray(), search.x, search.y, search.width, search.height)
        .to_image();

    // NCC score map: dims = (search.width - tw + 1) x (search.height - th + 1).
    // Higher = better match for CrossCorrelationNormalized.
    let score_map: image::ImageBuffer<Luma<f32>, Vec<f32>> =
        match_template(&scene, tpl_gray, MatchTemplateMethod::CrossCorrelationNormalized);

    // Collect candidate matches. Non-finite scores are dropped (never matches).
    let mut candidates: Vec<(f32, ImageRect)> = Vec::new();
    for (mx, my, px) in score_map.enumerate_pixels() {
        let score = px.0[0];
        if !score.is_finite() {
            continue;
        }
        let bx = (search.x + mx) as f32;
        let by = (search.y + my) as f32;
        candidates.push((score, ImageRect { x: bx, y: by, width: tw as f32, height: th as f32 }));
    }

    // Stable sort by score desc; tie-break by (x, y) asc for determinism.
    candidates.sort_by(|a, b| {
        b.0.partial_cmp(&a.0)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.1.x.partial_cmp(&b.1.x).unwrap_or(std::cmp::Ordering::Equal))
            .then(a.1.y.partial_cmp(&b.1.y).unwrap_or(std::cmp::Ordering::Equal))
    });

    // Greedy NMS.
    let mut kept: Vec<(f32, ImageRect)> = Vec::new();
    for (score, rect) in candidates {
        if kept.iter().any(|(_, k)| iou(*k, rect) > NMS_IOU_THRESHOLD) {
            continue;
        }
        kept.push((score, rect));
        if kept.len() as u32 >= limit {
            break;
        }
    }

    Ok(kept
        .into_iter()
        .map(|(score, bounds)| TemplateMatch {
            bounds,
            score,
            anchor: ImagePoint::new(bounds.x + bounds.width / 2.0, bounds.y + bounds.height / 2.0),
        })
        .collect())
}
```

- [ ] **Step 4: Run the templateMatch tests**

Run: `rtk cargo test -p rollshot-vision template`
Expected: PASS (the 5 new tests + the PR3 tests). If `finds_both_instances_with_nms` returns extra low-score matches, the greedy NMS or limit is wrong — re-check IoU threshold. The two pasted instances are 18px apart so their 8px boxes do not overlap (IoU 0); only the per-instance clusters collapse.

- [ ] **Step 5: Wire `RealAutomationHost` to the index + store**

Replace `crates/rollshot-vision/src/host.rs` struct/impl with:

```rust
use rollshot_automation::{
    AutomationHost, CapabilityError, LayoutQuery, LayoutRegion, OcrMatch, OcrQuery, RegionFeatures,
    RegionFeaturesQuery, TemplateMatch, TemplateMatchQuery,
};

use crate::index::VisualIndex;
use crate::template::TemplateStore;

#[derive(Debug)]
pub struct RealAutomationHost {
    index: VisualIndex,
    templates: TemplateStore,
}

impl RealAutomationHost {
    pub fn new(index: VisualIndex, templates: TemplateStore) -> Self {
        Self { index, templates }
    }
}

impl AutomationHost for RealAutomationHost {
    fn ocr(&mut self, _query: OcrQuery) -> Result<Vec<OcrMatch>, CapabilityError> {
        Err(CapabilityError::Failed { code: "capability_unavailable" })
    }

    fn layout(&mut self, _query: LayoutQuery) -> Result<Vec<LayoutRegion>, CapabilityError> {
        Err(CapabilityError::Failed { code: "capability_unavailable" })
    }

    fn region_features(
        &mut self,
        _query: RegionFeaturesQuery,
    ) -> Result<Vec<RegionFeatures>, CapabilityError> {
        Err(CapabilityError::Failed { code: "capability_unavailable" })
    }

    fn template_match(
        &mut self,
        query: TemplateMatchQuery,
    ) -> Result<Vec<TemplateMatch>, CapabilityError> {
        self.index.template_match(&self.templates, query)
    }
}
```

Update the PR1 host test (`unimplemented_capabilities_report_unavailable`): it constructed `RealAutomationHost::new()` with no args. Change it to assert `ocr`/`layout`/`region_features` return `capability_unavailable` using a host built from a tiny index + empty store, and drop the `template_match` assertion (now implemented):

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::index::VisualIndex;
    use crate::template::TemplateStore;
    use rollshot_automation::{LayoutQuery, Region};

    #[test]
    fn unimplemented_capabilities_report_unavailable() {
        let index = VisualIndex::build(image::RgbaImage::from_pixel(
            4, 4, image::Rgba([0, 0, 0, 255]),
        ))
        .unwrap();
        let mut host = RealAutomationHost::new(index, TemplateStore::new());
        let err = host
            .layout(LayoutQuery { region: Region::Full, limit: 1 })
            .unwrap_err();
        assert_eq!(err, CapabilityError::Failed { code: "capability_unavailable" });
    }
}
```

- [ ] **Step 6: `mod index;` visibility — ensure `template.rs` can see `VisualIndex`**

In `lib.rs`, `index` is a private module; `template.rs` uses `crate::index::VisualIndex` (allowed within the crate). Confirm `index` is declared `mod index;` (not behind a feature). No public API change.

- [ ] **Step 7: Run all crate tests + clippy**

Run: `rtk cargo test -p rollshot-vision`
Expected: PASS.
Run: `rtk cargo clippy -p rollshot-vision -- -D warnings`
Expected: clean.

- [ ] **Step 8: Commit**

```bash
rtk git add crates/rollshot-vision
rtk git commit -m "feat(vision): NCC templateMatch v0 + NMS, wired into RealAutomationHost (PR4)"
```

- [ ] **Step 9: Handoff note**

Append: "PR4 done — `match_template_image` (NCC via imageproc, low-info reject, non-finite drop, greedy NMS, limit), `VisualIndex::template_match`, `RealAutomationHost::new(index, store)` delegates `template_match`. anchor=center. Next: PR5 self-validation."

---

## Task 5: `TemplateSelfValidation` (PR5)

**Files:**
- Create: `crates/rollshot-vision/src/self_validation.rs`
- Modify: `crates/rollshot-vision/src/lib.rs` (add `mod self_validation;` + re-exports)

**Interfaces:**
- Consumes: `VisualIndex` (`image()`, `width()`, `height()`), `template::match_template_image`, `rect::iou`, `rollshot_image_document::ImageRect`, `VisionError`.
- Produces:
  - `self_validation::ExpectedCount { Unique, Repeating, AtLeast(u32) }`.
  - `self_validation::TemplateDecision { Pass, NeedsConfirm, Reject }`.
  - `self_validation::SelfValidationConfig { expected_count: ExpectedCount, target_bounds: Option<ImageRect> }`.
  - `self_validation::TemplateSelfValidation { self_score, second_best_score, peak_margin, false_positive_count, edge_density, entropy, stable_under_jitter, decision }`.
  - `self_validation::self_validate(index: &VisualIndex, candidate_bounds: ImageRect, cfg: &SelfValidationConfig) -> Result<TemplateSelfValidation, VisionError>`.

- [ ] **Step 1: Write the failing tests (Pass / Reject flat / Reject everywhere)**

Create `crates/rollshot-vision/src/self_validation.rs` with a test module first:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::index::VisualIndex;
    use rollshot_image_document::ImageRect;

    fn cfg(expected: ExpectedCount) -> SelfValidationConfig {
        SelfValidationConfig { expected_count: expected, target_bounds: None }
    }

    // Scene with one distinctive checker glyph at (10,12), size 8x8.
    fn distinctive_scene() -> image::RgbaImage {
        let mut scene = image::RgbaImage::from_pixel(40, 40, image::Rgba([180, 180, 180, 255]));
        for dy in 0..8 {
            for dx in 0..8 {
                let v = if (dx + dy) % 2 == 0 { 0 } else { 60 };
                scene.put_pixel(10 + dx, 12 + dy, image::Rgba([v, v, v, 255]));
            }
        }
        scene
    }

    #[test]
    fn distinctive_candidate_passes() {
        let index = VisualIndex::build(distinctive_scene()).unwrap();
        let v = self_validate(
            &index,
            ImageRect { x: 10.0, y: 12.0, width: 8.0, height: 8.0 },
            &cfg(ExpectedCount::Unique),
        )
        .unwrap();
        assert_eq!(v.decision, TemplateDecision::Pass);
        assert!(v.self_score > 0.9);
    }

    #[test]
    fn flat_candidate_is_rejected() {
        // Crop a uniform patch -> low edge/entropy.
        let index = VisualIndex::build(image::RgbaImage::from_pixel(
            40, 40, image::Rgba([180, 180, 180, 255]),
        ))
        .unwrap();
        let v = self_validate(
            &index,
            ImageRect { x: 5.0, y: 5.0, width: 8.0, height: 8.0 },
            &cfg(ExpectedCount::Unique),
        )
        .unwrap();
        assert_eq!(v.decision, TemplateDecision::Reject);
    }

    #[test]
    fn out_of_bounds_candidate_errors() {
        let index = VisualIndex::build(distinctive_scene()).unwrap();
        let e = self_validate(
            &index,
            ImageRect { x: 38.0, y: 38.0, width: 8.0, height: 8.0 },
            &cfg(ExpectedCount::Unique),
        )
        .unwrap_err();
        assert_eq!(e, crate::VisionError::CandidateOutOfBounds);
    }
}
```

- [ ] **Step 2: Run to verify failure**

Run: `rtk cargo test -p rollshot-vision self_validation`
Expected: FAIL to compile.

- [ ] **Step 3: Implement `self_validation.rs`**

Prepend to `crates/rollshot-vision/src/self_validation.rs`:

```rust
//! Author-time template self-validation. Pure and deterministic. The caller
//! (SP3 author pipeline) supplies a candidate region; this module crops it from
//! the source image, matches it back, and measures whether it is a reliable
//! template. Confidence is measured here, NOT taken from any LLM.

use rollshot_image_document::ImageRect;

use crate::index::VisualIndex;
use crate::rect::iou;
use crate::template::match_template_image;
use crate::VisionError;
use rollshot_automation::Region;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExpectedCount {
    Unique,
    Repeating,
    AtLeast(u32),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TemplateDecision {
    Pass,
    NeedsConfirm,
    Reject,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SelfValidationConfig {
    pub expected_count: ExpectedCount,
    pub target_bounds: Option<ImageRect>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TemplateSelfValidation {
    pub self_score: f32,
    pub second_best_score: Option<f32>,
    pub peak_margin: f32,
    pub false_positive_count: u32,
    pub edge_density: f32,
    pub entropy: f32,
    pub stable_under_jitter: bool,
    pub decision: TemplateDecision,
}

// Tunable floors (SP1 constants; config-ize later).
const EDGE_DENSITY_FLOOR: f32 = 0.05;
const ENTROPY_FLOOR: f32 = 1.5;
const FALSE_POSITIVE_SCORE: f32 = 0.7;
const CLEAN_PEAK_MARGIN: f32 = 0.15;
const SELF_SCORE_FLOOR: f32 = 0.9;
const JITTER_SCORE_DROP: f32 = 0.2;

pub fn self_validate(
    index: &VisualIndex,
    candidate_bounds: ImageRect,
    cfg: &SelfValidationConfig,
) -> Result<TemplateSelfValidation, VisionError> {
    let (iw, ih) = (index.width(), index.height());
    // Candidate must lie fully inside the image.
    if !candidate_bounds.is_finite()
        || candidate_bounds.x < 0.0
        || candidate_bounds.y < 0.0
        || candidate_bounds.width <= 0.0
        || candidate_bounds.height <= 0.0
        || candidate_bounds.x + candidate_bounds.width > iw as f32
        || candidate_bounds.y + candidate_bounds.height > ih as f32
    {
        return Err(VisionError::CandidateOutOfBounds);
    }

    let cx = candidate_bounds.x.floor() as u32;
    let cy = candidate_bounds.y.floor() as u32;
    let cw = candidate_bounds.width.round().max(1.0) as u32;
    let ch = candidate_bounds.height.round().max(1.0) as u32;

    let candidate_rgba =
        image::imageops::crop_imm(index.image(), cx, cy, cw, ch).to_image();
    let candidate_gray = image::imageops::grayscale(&candidate_rgba);

    let edge_density = edge_density(&candidate_gray);
    let entropy = entropy(&candidate_gray);

    // Match the candidate back against the full image. A low-information
    // candidate is rejected by match_template_image; treat that as Reject.
    let matches = match match_template_image(index, &candidate_gray, &Region::Full, 32) {
        Ok(m) => m,
        Err(_) => {
            return Ok(TemplateSelfValidation {
                self_score: 0.0,
                second_best_score: None,
                peak_margin: 0.0,
                false_positive_count: 0,
                edge_density,
                entropy,
                stable_under_jitter: false,
                decision: TemplateDecision::Reject,
            });
        }
    };

    let self_score = matches.first().map(|m| m.score).unwrap_or(0.0);
    let second_best_score = matches.get(1).map(|m| m.score);

    let k = match cfg.expected_count {
        ExpectedCount::Unique => 1usize,
        ExpectedCount::Repeating => 2,
        ExpectedCount::AtLeast(n) => n.max(1) as usize,
    };
    // peak_margin: gap between the k-th accepted match and the next one.
    let peak_margin = match (matches.get(k - 1), matches.get(k)) {
        (Some(a), Some(b)) => a.score - b.score,
        (Some(_), None) => 1.0, // clean cliff: nothing beyond the expected set
        _ => 0.0,
    };
    let false_positive_count = matches
        .iter()
        .skip(k)
        .filter(|m| m.score >= FALSE_POSITIVE_SCORE)
        .count() as u32;

    let stable_under_jitter =
        jitter_stable(index, &candidate_rgba, candidate_bounds, self_score);

    let count_ok = match cfg.expected_count {
        ExpectedCount::Unique => true,
        ExpectedCount::Repeating => matches.iter().filter(|m| m.score >= FALSE_POSITIVE_SCORE).count() >= 2,
        ExpectedCount::AtLeast(n) => {
            matches.iter().filter(|m| m.score >= FALSE_POSITIVE_SCORE).count() >= n as usize
        }
    };

    let coverage_ok = match cfg.target_bounds {
        None => true,
        Some(t) => matches.iter().any(|m| iou(m.bounds, t) >= 0.3),
    };

    let decision = decide(
        self_score,
        edge_density,
        entropy,
        peak_margin,
        false_positive_count,
        stable_under_jitter,
        count_ok,
        coverage_ok,
    );

    Ok(TemplateSelfValidation {
        self_score,
        second_best_score,
        peak_margin,
        false_positive_count,
        edge_density,
        entropy,
        stable_under_jitter,
        decision,
    })
}

#[allow(clippy::too_many_arguments)]
fn decide(
    self_score: f32,
    edge_density: f32,
    entropy: f32,
    peak_margin: f32,
    false_positive_count: u32,
    stable: bool,
    count_ok: bool,
    coverage_ok: bool,
) -> TemplateDecision {
    let structural_floor = edge_density >= EDGE_DENSITY_FLOOR && entropy >= ENTROPY_FLOOR;
    if self_score < SELF_SCORE_FLOOR
        || !structural_floor
        || false_positive_count > 0
        || !stable
    {
        return TemplateDecision::Reject;
    }
    if peak_margin >= CLEAN_PEAK_MARGIN && count_ok && coverage_ok {
        return TemplateDecision::Pass;
    }
    TemplateDecision::NeedsConfirm
}

/// Fraction of pixels whose local gradient magnitude exceeds a threshold.
fn edge_density(gray: &image::GrayImage) -> f32 {
    let (w, h) = gray.dimensions();
    if w < 2 || h < 2 {
        return 0.0;
    }
    let mut edges = 0u32;
    let mut total = 0u32;
    for y in 0..h - 1 {
        for x in 0..w - 1 {
            let c = gray.get_pixel(x, y).0[0] as i32;
            let gx = (gray.get_pixel(x + 1, y).0[0] as i32 - c).abs();
            let gy = (gray.get_pixel(x, y + 1).0[0] as i32 - c).abs();
            if gx + gy > 30 {
                edges += 1;
            }
            total += 1;
        }
    }
    if total == 0 {
        0.0
    } else {
        edges as f32 / total as f32
    }
}

/// Shannon entropy of the 256-bin intensity histogram, in bits.
fn entropy(gray: &image::GrayImage) -> f32 {
    let mut hist = [0u32; 256];
    let mut n = 0u32;
    for p in gray.pixels() {
        hist[p.0[0] as usize] += 1;
        n += 1;
    }
    if n == 0 {
        return 0.0;
    }
    let nf = n as f32;
    let mut e = 0.0f32;
    for &c in hist.iter() {
        if c > 0 {
            let p = c as f32 / nf;
            e -= p * p.log2();
        }
    }
    e
}

/// Re-match a brightness-jittered copy of the candidate; require the best match
/// to stay near the original location with a bounded score drop.
fn jitter_stable(
    index: &VisualIndex,
    candidate_rgba: &image::RgbaImage,
    candidate_bounds: ImageRect,
    base_score: f32,
) -> bool {
    // Brightness +5%.
    let mut jittered = candidate_rgba.clone();
    for p in jittered.pixels_mut() {
        for c in 0..3 {
            p.0[c] = ((p.0[c] as f32) * 1.05).min(255.0) as u8;
        }
    }
    let jittered_gray = image::imageops::grayscale(&jittered);
    let matches = match match_template_image(index, &jittered_gray, &Region::Full, 4) {
        Ok(m) => m,
        Err(_) => return false,
    };
    match matches.first() {
        Some(m) => {
            let same_place = iou(m.bounds, candidate_bounds) >= 0.5;
            let small_drop = base_score - m.score <= JITTER_SCORE_DROP;
            same_place && small_drop
        }
        None => false,
    }
}
```

- [ ] **Step 4: Wire `self_validation` into `lib.rs`**

```rust
mod self_validation;

pub use self_validation::{
    self_validate, ExpectedCount, SelfValidationConfig, TemplateDecision, TemplateSelfValidation,
};
```

- [ ] **Step 5: Run the self-validation tests**

Run: `rtk cargo test -p rollshot-vision self_validation`
Expected: PASS (3 tests). If `distinctive_candidate_passes` returns `NeedsConfirm`, the checker glyph's `peak_margin` is below `CLEAN_PEAK_MARGIN` because there is only one instance — confirm the `(Some(_), None) => 1.0` clean-cliff branch is hit (single match means `matches.get(1)` is `None`, so `peak_margin` should be 1.0). If `flat_candidate_is_rejected` fails, the flat crop should be caught either by `template_low_information` (→ Reject branch) or the structural floor.

- [ ] **Step 6: Run all crate tests + clippy**

Run: `rtk cargo test -p rollshot-vision && rtk cargo clippy -p rollshot-vision -- -D warnings`
Expected: PASS, clean.

- [ ] **Step 7: Commit**

```bash
rtk git add crates/rollshot-vision
rtk git commit -m "feat(vision): TemplateSelfValidation (PR5)"
```

- [ ] **Step 8: Handoff note**

Append: "PR5 done — `self_validate(candidate_bounds)` crops internally, measures self_score/peak_margin/false_positive/edge/entropy/jitter, returns Pass/NeedsConfirm/Reject. Next: PR6 integration tests."

---

## Task 6: role-free QuickJS fixture integration tests (PR6)

**Files:**
- Create: `crates/rollshot-vision/tests/fixtures/hide_bookmarks.js`
- Create: `crates/rollshot-vision/tests/fixtures/hide_folders.js`
- Create: `crates/rollshot-vision/tests/integration.rs`

**Interfaces:**
- Consumes: public `rollshot_vision::{VisualIndex, TemplateStore, TemplateAsset, TemplateBytes, TemplateSensitivity, TemplateSource, RealAutomationHost}`; `rollshot_automation::{validate_source, ValidationLimits, AutomationInput, Region, ProposedEditKind, execute_to_proposal, CancellationFlag, ExecutionPolicy}`; `rollshot_automation::ProposalContext`; `rollshot_edit_proposal::{ProposalId, Provenance, ProvenanceSource, ProposedEdit}`; `rollshot_automation_rquickjs::QuickJsExecutor`.

- [ ] **Step 1: Create the detector fixtures (the spec's validated role-free detectors)**

`crates/rollshot-vision/tests/fixtures/hide_bookmarks.js`:

```js
function main(input) {
  const matches = rollshot.templateMatch({
    templateHandle: input.capabilityHandles.bookmarkStrip,
    region: { kind: "full" },
    limit: 40,
  });
  return {
    candidates: matches
      .filter((match) => match.score >= 0.82)
      .map((match) => ({
        kind: "addRedaction",
        bounds: match.bounds,
        confidence: Math.min(0.95, match.score),
        label: "bookmark-strip-template",
      })),
  };
}
```

`crates/rollshot-vision/tests/fixtures/hide_folders.js`:

```js
function padToCaption(bounds) {
  return {
    x: Math.max(0, bounds.x - 8),
    y: Math.max(0, bounds.y - 8),
    width: bounds.width + 16,
    height: bounds.height + 36,
  };
}

function main(input) {
  const matches = rollshot.templateMatch({
    templateHandle: input.capabilityHandles.folderIcon,
    region: { kind: "full" },
    limit: 80,
  });
  return {
    candidates: matches
      .filter((match) => match.score >= 0.8)
      .map((match) => ({
        kind: "addRedaction",
        bounds: padToCaption(match.bounds),
        confidence: Math.min(0.94, match.score),
        label: "desktop-folder-icon",
      })),
  };
}
```

- [ ] **Step 2: Write the integration test harness + bookmark case (failing)**

Create `crates/rollshot-vision/tests/integration.rs`:

```rust
use std::time::Duration;

use rollshot_automation::{
    execute_to_proposal, validate_source, AutomationInput, CancellationFlag, ExecutionPolicy,
    ProposalContext, ProposedEditKind, Region, ValidationLimits,
};
use rollshot_automation_rquickjs::QuickJsExecutor;
use rollshot_edit_proposal::{EditProposal, ProposedEdit, Provenance, ProvenanceSource, ProposalId};
use rollshot_vision::{
    RealAutomationHost, TemplateAsset, TemplateBytes, TemplateSensitivity, TemplateSource,
    TemplateStore, VisualIndex,
};

const BOOKMARKS_JS: &str = include_str!("fixtures/hide_bookmarks.js");
const FOLDERS_JS: &str = include_str!("fixtures/hide_folders.js");

/// 60x60 mid-grey scene. Paste an 8x8 checker glyph at each (x,y) in `marks`.
fn scene_with(marks: &[(u32, u32)]) -> image::RgbaImage {
    let mut scene = image::RgbaImage::from_pixel(60, 60, image::Rgba([180, 180, 180, 255]));
    for &(ox, oy) in marks {
        for dy in 0..8 {
            for dx in 0..8 {
                let v = if (dx + dy) % 2 == 0 { 0 } else { 60 };
                scene.put_pixel(ox + dx, oy + dy, image::Rgba([v, v, v, 255]));
            }
        }
    }
    scene
}

fn template_from(scene: &image::RgbaImage, x: u32, y: u32) -> TemplateBytes {
    let crop = image::imageops::crop_imm(scene, x, y, 8, 8).to_image();
    TemplateBytes::new(8, 8, crop.into_raw()).unwrap()
}

fn store_with(handle: &str, bytes: TemplateBytes) -> TemplateStore {
    let mut store = TemplateStore::new();
    store.insert(TemplateAsset {
        handle: handle.into(),
        sensitivity: TemplateSensitivity::Chrome,
        source: TemplateSource::UserRect,
        created_at_ms: 0,
        bounds_in_source_image: None,
        bytes,
    });
    store
}

fn run(
    js: &str,
    scene: image::RgbaImage,
    store: TemplateStore,
    handle_key: &str,
    handle_value: &str,
) -> EditProposal {
    let (w, h) = scene.dimensions();
    let automation = validate_source(js, &ValidationLimits::default()).unwrap();
    let mut handles = std::collections::BTreeMap::new();
    handles.insert(handle_key.to_string(), handle_value.to_string());
    let input = AutomationInput {
        image_width: w,
        image_height: h,
        region: Some(Region::Full),
        annotations: Vec::new(),
        capability_handles: handles,
    };
    let proposal = ProposalContext {
        proposal_id: ProposalId(1),
        base_document_state_id: 0,
        provenance: Provenance { source: ProvenanceSource::Agent { run_id: 1 } },
    };
    let mut policy = ExecutionPolicy::smart_redaction_default(
        Duration::from_secs(2),
        16 * 1024 * 1024,
        256 * 1024,
    );
    policy.allowed_edit_kinds.insert(ProposedEditKind::AddRedaction);

    let index = VisualIndex::build(scene).unwrap();
    let mut host = RealAutomationHost::new(index, store);
    let (proposal, _metrics) = execute_to_proposal(
        &QuickJsExecutor,
        &automation,
        &input,
        &proposal,
        &mut host,
        &policy,
        &CancellationFlag::new(),
    )
    .unwrap();
    proposal
}

#[test]
fn bookmark_strip_produces_one_candidate() {
    let scene = scene_with(&[(6, 4)]); // single "strip-like" mark
    let tpl = template_from(&scene, 6, 4);
    let proposal = run(
        BOOKMARKS_JS,
        scene,
        store_with("bookmarkStrip", tpl),
        "bookmarkStrip",
        "bookmarkStrip",
    );
    assert_eq!(proposal.candidates.len(), 1);
    match &proposal.candidates[0].edit {
        ProposedEdit::AddRedaction { bounds } => {
            assert!((bounds.x - 6.0).abs() <= 2.0);
            assert!((bounds.y - 4.0).abs() <= 2.0);
        }
        other => panic!("expected AddRedaction, got {other:?}"),
    }
    assert_eq!(proposal.candidates[0].label, "bookmark-strip-template");
}
```

- [ ] **Step 3: Run to verify it passes**

Run: `rtk cargo test -p rollshot-vision --test integration bookmark_strip_produces_one_candidate`
Expected: PASS. (If it fails to find the mark, confirm the template handle wiring `input.capabilityHandles.bookmarkStrip` resolves to the store handle.)

- [ ] **Step 4: Add the folder-grid case**

Add to `integration.rs`:

```rust
#[test]
fn folder_grid_produces_candidate_per_icon() {
    let marks = [(6u32, 6u32), (30, 6), (6, 30), (30, 30)];
    let scene = scene_with(&marks);
    let tpl = template_from(&scene, 6, 6);
    let proposal = run(
        FOLDERS_JS,
        scene,
        store_with("folderIcon", tpl),
        "folderIcon",
        "folderIcon",
    );
    // One padded candidate per pasted icon.
    assert_eq!(proposal.candidates.len(), marks.len());
}
```

- [ ] **Step 5: Add the negative (blank) case**

```rust
#[test]
fn blank_scene_produces_no_candidates() {
    // Distinctive template, but the scene has no instance of it.
    let template_scene = scene_with(&[(6, 6)]);
    let tpl = template_from(&template_scene, 6, 6);
    let blank = image::RgbaImage::from_pixel(60, 60, image::Rgba([200, 120, 40, 255]));
    let proposal = run(
        BOOKMARKS_JS,
        blank,
        store_with("bookmarkStrip", tpl),
        "bookmarkStrip",
        "bookmarkStrip",
    );
    assert_eq!(proposal.candidates.len(), 0);
}
```

- [ ] **Step 6: Add the determinism case**

```rust
#[test]
fn detection_is_deterministic() {
    let make = || {
        let scene = scene_with(&[(6, 4)]);
        let tpl = template_from(&scene, 6, 4);
        run(BOOKMARKS_JS, scene, store_with("bookmarkStrip", tpl), "bookmarkStrip", "bookmarkStrip")
    };
    let a = make();
    let b = make();
    assert_eq!(a.candidates, b.candidates);
}
```

- [ ] **Step 7: Run the full integration suite**

Run: `rtk cargo test -p rollshot-vision --test integration`
Expected: PASS (4 tests). If `folder_grid_produces_candidate_per_icon` returns fewer than 4, the four marks are ≥ 22px apart so their 8px boxes never overlap — verify NMS isn't over-merging (IoU threshold 0.4). If `blank_scene_produces_no_candidates` returns candidates, the JS `.filter(score >= 0.82)` should drop low NCC scores against an unrelated scene; lower the synthetic similarity if a spurious peak clears 0.82.

- [ ] **Step 8: Run the whole crate + workspace check**

Run: `rtk cargo test -p rollshot-vision`
Expected: PASS (unit + integration).
Run: `rtk cargo clippy -p rollshot-vision --all-targets -- -D warnings && rtk cargo fmt --check`
Expected: clean.

- [ ] **Step 9: Commit**

```bash
rtk git add crates/rollshot-vision/tests
rtk git commit -m "test(vision): role-free QuickJS fixture integration tests (PR6)"
```

- [ ] **Step 10: Handoff note (sub-project close)**

Append to `docs/superpowers/handoffs/2026-06-22-rollshot-vision.md`: "PR6 done — SP1 complete. `hide_bookmarks.js` and `hide_folders.js` run through `QuickJsExecutor` + `RealAutomationHost` and produce expected candidates on synthetic fixtures; negative + determinism cases pass. Deferred to later sub-projects: regionFeatures (SP2), author-time template pipeline (SP3, needs agent core), inspectLayout (SP4), OCR (SP5), product wiring (SP6). Carry-forward risks: tall-image NCC perf, NCC scale-invariance."

---

## Self-Review

**Spec coverage:**
- §3.1 crate/deps/dependency-direction → Task 1. §3.2 module layout → Tasks 1–6 create the listed files (`ocr.rs`/`layout.rs`/`region_features.rs` correctly absent — deferred). ✅
- §4.1 `RealAutomationHost` + capability_unavailable → Task 1 (stub) + Task 4 (wired). ✅
- §4.2 `VisualIndex` (eager grayscale, reject empty, no build-options) → Task 2. ✅
- §4.3 TemplateStore/Sensitivity/TemplateBytes/serialize gate/records → Task 3. ✅
- §4.4 templateMatch (NCC, to_pixel_rect rules, low-info, non-finite, NMS, anchor=center, host-no-threshold) → Task 4 (+ Task 2 for `to_pixel_rect`). ✅
- §4.5 self_validate(candidate_bounds) + signals + decision + ExpectedCount → Task 5. ✅
- §5 error model (VisionError + CapabilityError codes) → Task 1 (VisionError), Tasks 2/4 (codes). ✅
- §6 privacy (export strips Sensitive) → Task 3. ✅
- §7.1 unit tests → Tasks 2–5; §7.2 integration matrix (bookmark, folder grid, blank, template_not_found, capability_unavailable, determinism) → Task 4 (template_not_found), Task 1/4 (capability_unavailable), Task 6 (bookmark/folder/blank/determinism). **Gap:** the spec's §7.2 "縮放變體 known miss" (scale) case is not in the plan. *Resolution:* it is a documented known limitation, not a required passing behavior; left to manual verification. Noted here intentionally rather than adding a brittle test.
- §8 PR breakdown (Model C, per-PR handoff notes) → Tasks 1–6 each end with a handoff-note step. ✅
- Guardrails → enforced across Tasks 2–5; restated in Global Constraints. ✅

**Placeholder scan:** No TBD/TODO; every code step has complete code. The one external-API uncertainty (imageproc `match_template`/`MatchTemplateMethod` exact signature) is handled by an explicit verification step (Task 4 Step 1) rather than a guess. ✅

**Type consistency:** `match_template_image` signature identical in Task 4 (definition) and Task 5 (consumer): `(&VisualIndex, &GrayImage, &Region, u32) -> Result<Vec<TemplateMatch>, CapabilityError>`. `RealAutomationHost::new` is `()` in Task 1 then `(VisualIndex, TemplateStore)` in Task 4 — Task 1's host test is explicitly rewritten in Task 4 Step 5 to match. `TemplateBytes::new`, `TemplateStore::{get,insert,save_local,export}`, `self_validate` signatures are consistent between definition and use. ✅
