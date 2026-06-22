# rollshot-vision Runtime Host (Sub-project 1) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build `crates/rollshot-vision`, an agent-independent, deterministic, template-first runtime detection host that implements `AutomationHost`, so hand-authored template detectors produce real redaction candidates through the existing QuickJS executor.

**Architecture:** A new pure-Rust crate provides `VisualIndex` (built once per run, holds the image + cached grayscale), a bounded `TemplateStore` with privacy-tagged `TemplateAsset`s, normalized-correlation template preparation with deterministic peak extraction/NMS, and `TemplateSelfValidation`. Expensive vision work runs before QuickJS execution through `RealAutomationHost::prepare_template_match`; the `AutomationHost` callback only validates the prepared query, looks up cached results, and truncates to `limit` so it remains inside the existing synchronous-runtime contract. Unimplemented capabilities return an explicit `capability_unavailable` error rather than empty results.

**Tech Stack:** Rust 2021 with workspace MSRV 1.94, `image` 0.25, `imageproc` 0.26.2-compatible API (`default-features = false`), `rollshot-automation` (trait + capability types), `rollshot-image-document` (geometry), and `rollshot-edit-proposal` (proposal output). Tests use `rollshot-automation-rquickjs` as a dev-dependency.

**Spec:** `docs/superpowers/specs/2026-06-22-rollshot-vision-runtime-host-design.md`

## Engineering Review Lock (2026-06-22, auto mode)

### Step 0 — Scope challenge

- **Goal alignment:** all six tasks contribute to the SP1 goal. `TemplateSelfValidation` is author-time support, but it is explicitly part of the approved SP1 spec and is retained.
- **Minimum viable scope:** Tasks 1–4 + Task 6 are the runtime demo slice. Task 5 could technically move to SP3, but doing so would violate the approved SP1 boundary and leave the privacy-sensitive template-acquisition contract untested; keep it.
- **Complexity check:** 12 net-new files including the handoff document, one new crate, six tasks. The review threshold is not triggered.
- **Distribution check:** `rollshot-vision` is a workspace library, not a separately installed artifact. Workspace CI/build coverage is required; packaging and product feature wiring remain SP6.
- **Search check:** `imageproc` already provides raw template cross-correlation, so SP1 uses it instead of adding OpenCV or another native stack. Its `CrossCorrelationNormalized` is normalized dot-product correlation rather than zero-mean NCC and can over-score unrelated positive-intensity windows; SP1 therefore uses raw `CrossCorrelation` plus integral window moments to produce true zero-mean NCC. The library also requires template dimensions to be strictly smaller than the image, and the parallel entry point is unavailable with `default-features = false`; the plan names and tests those constraints.
- **Completeness check:** serialization round-trip, corruption handling, exact query preparation, invalid regions, equal-size matching, known scale miss, callback cache misses, deterministic ordering, self-validation gates, and resource refusals are required automated coverage rather than manual follow-up.

### Locked runtime data flow

```text
RgbaImage + TemplateStore + concrete TemplateMatchQuery values
        |
        v
VisualIndex::build
        |
        v
RealAutomationHost::prepare_template_match
  - validate the concrete query and region
  - enforce search-position + pixel-visit budgets
  - run imageproc matching outside QuickJS
  - bounded peak extraction + deterministic NMS
  - cache results by handle + exact prepared region
        |
        v
QuickJsExecutor
        |
        v
AutomationHost::template_match callback (< 1 ms target)
  - validate prepared query
  - cached lookup
  - truncate to requested limit
        |
        v
strict output decode -> EditProposal -> human review
```

General extraction of preparation queries from arbitrary `ValidatedAutomation` is not added in SP1. The PR6 hand-authored fixtures pass their concrete query to preparation explicitly. SP6 product wiring must either derive a preparation plan before execution or introduce a separately reviewed automation-contract extension; it must not move detector work back into the QuickJS callback.

### What already exists

| Existing code / flow | Reuse decision |
|---|---|
| `rollshot-automation::AutomationHost` and capability DTOs | Reuse unchanged; `rollshot-vision` implements the trait. |
| `rollshot-automation-rquickjs` bridge validation, result truncation, host-allocation accounting, and typed capability propagation | Reuse; do not duplicate sandbox or output checks. |
| `validate_source` → Workflow IR/manifest → `execute_to_proposal` | Reuse for PR6 end-to-end coverage. |
| `rollshot-edit-proposal` policy validation and transient candidate model | Reuse; vision never mutates `ImageDocument`. |
| `rollshot-image-document::ImageRect` / `ImagePoint` | Reuse geometry types; add vision-specific integer conversion only. |
| `rollshot-core` NCC/matcher knowledge and metrics | Reference its cost/low-variance discipline, but do not depend on private stitching matcher internals or create a reverse dependency. |
| `imageproc::template_matching` | Reuse the library primitive; wrap its dimension, resource, score, and determinism footguns. |

### NOT in scope

- Product/Result Workspace wiring, UI disclosure, candidate editing, and safe-save handoff — SP6 owns the user-facing flow.
- Automatic extraction of preparation queries from arbitrary validated JavaScript — requires an automation-contract/product-wiring decision, not a hidden SP1 parser.
- `regionFeatures`, author-time `inspectLayout`, OCR, OpenCV, object detection, and scale/rotation-invariant matching — retained in SP2/SP4/SP5/optional follow-ups.
- General preset persistence, revision storage, sync, sharing, encryption-at-rest, and interactive export confirmation — SP1 defines and tests explicit local/export record serialization only.
- Claiming that a successful detector found all sensitive content — the product remains review-first.
- Benchmarking or changing `rollshot-core` stitching algorithms — this crate is a separate vision path.

### Execution strategy

Sequential execution, no parallelization opportunity. Task 1 changes the workspace root; Tasks 2–6 all touch `crates/rollshot-vision/`, and every task updates the same handoff document. Do not use worktrees.

## Global Constraints

Every task implicitly includes the approved spec constraints plus the review locks below:

- **Crate is `unsafe_code = "forbid"`** — pure image processing, no FFI. Inherit workspace lints (`[lints] workspace = true`).
- **`imageproc` is pinned at `0.26` with `default-features = false`** in `[workspace.dependencies]`; both `rollshot-core` and `rollshot-vision` use `imageproc = { workspace = true }`. No OCR/OpenCV native dependency.
- **`image = 0.25`** via `{ workspace = true }`. Inherit `edition.workspace = true` and `rust-version.workspace = true` (current workspace MSRV: 1.94); do not hard-code an older floor.
- **Capability boundary unchanged** — do not modify `rollshot-automation` public contracts. SP1 only adds a host implementation.
- **Errors:** build/store-time → `VisionError`; capability-call-time → `rollshot_automation::CapabilityError`. Capability rejection codes: `template_not_found`, `template_larger_than_region`, `region_too_large`, `non_finite_region`, `empty_region`, `template_low_information`, `capability_unavailable`, `vision_index_unavailable`.
- **Synchronous runtime contract:** never perform template matching, image conversion, disk IO, or another blocking detector operation inside an `AutomationHost` callback. Preparation happens before `execute_to_proposal`; callbacks perform bounded validation + cached lookup only.
- **Resource bounds:** enforce template count/total-byte limits plus checked search-position and template-pixel-visit budgets before allocating a score map or starting matching. Cost refusal uses `region_too_large` to stay inside Capability API v1.
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
- **Run/Expected pairing:** each test-writing step contains its own RED command, and each implementation/edit step is verified by the immediately following named GREEN or verification step. Do not advance a task when that paired command differs from its Expected result.

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
                        #   TemplateStore, Local/Export records, prepared matching, NMS
    self_validation.rs  # ExpectedCount, TemplateDecision, SelfValidationConfig,
                        #   TemplateSelfValidation, self_validate
    host.rs             # RealAutomationHost (prepared result cache + impl AutomationHost)
  tests/
    fixtures/
      hide_bookmarks.js
      hide_folders.js
    integration.rs      # PR6: real JS through QuickJsExecutor + RealAutomationHost
docs/superpowers/handoffs/
  2026-06-22-rollshot-vision.md
```

Constants live where used: search geometry limits in `rect.rs`; template/store/work limits in `template.rs` (documented module consts). Every multiplication uses checked `u64`/`usize` arithmetic before allocation.

---

## Task 1: Crate skeleton + workspace wiring (PR1)

**Files:**
- Modify: `Cargo.toml` (workspace root — add member + `imageproc` workspace dep)
- Modify: `crates/rollshot-core/Cargo.toml` (switch `imageproc` to workspace)
- Create: `crates/rollshot-vision/Cargo.toml`
- Create: `crates/rollshot-vision/src/lib.rs`
- Create: `crates/rollshot-vision/src/error.rs`
- Create: `crates/rollshot-vision/src/host.rs`
- Create: `docs/superpowers/handoffs/2026-06-22-rollshot-vision.md`

**Interfaces:**
- Produces: `rollshot_vision::VisionError` (enum); `rollshot_vision::RealAutomationHost` with `RealAutomationHost::new() -> Self` (PR1 stub, no fields) implementing `rollshot_automation::AutomationHost` where all four methods return `Err(CapabilityError::Failed { code: "capability_unavailable" })`.

- [ ] **Step 1: Pin `imageproc` in workspace dependencies**

Edit the workspace root `Cargo.toml` and add this line to `[workspace.dependencies]`:

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

Create the manifest first, then add `"crates/rollshot-vision"` to root `[workspace.members]` (keep the existing ordering). This order avoids a temporarily invalid workspace member during Step 3.

```toml
[package]
name = "rollshot-vision"
version = "0.1.0"
edition.workspace = true
rust-version.workspace = true

[lints]
workspace = true

[dependencies]
image = { workspace = true }
imageproc = { workspace = true }
rollshot-automation = { path = "../rollshot-automation" }
rollshot-image-document = { path = "../rollshot-image-document", features = ["serde"] }
serde = { workspace = true }
serde_json = { workspace = true }
thiserror = { workspace = true }
tracing = { workspace = true }

[dev-dependencies]
rollshot-automation-rquickjs = { path = "../rollshot-automation-rquickjs" }
static_assertions = "1.1"
```

`serde_json` supports explicit local/export record persistence. `static_assertions` locks the privacy rule that asset/store types never implement `Serialize`.

- [ ] **Step 5: Write the host contract test before the implementation**

Create `crates/rollshot-vision/src/lib.rs` with the crate-level docs, `#![forbid(unsafe_code)]`, and this test-only contract:

```rust
//! Rollshot-specific, deterministic, UI-oriented vision adapter layer.

#![forbid(unsafe_code)]

#[cfg(test)]
mod contract_tests {
    use rollshot_automation::{
        AutomationHost, LayoutQuery, OcrQuery, Region, RegionFeaturesQuery, TemplateMatchQuery,
    };

    use crate::RealAutomationHost;

    #[test]
    fn all_unimplemented_capabilities_report_unavailable() {
        let mut host = RealAutomationHost::new();
        let expected = rollshot_automation::CapabilityError::Failed {
            code: "capability_unavailable",
        };
        assert_eq!(
            host.ocr(OcrQuery { region: Region::Full, limit: 1 }).unwrap_err(),
            expected
        );
        assert_eq!(
            host.layout(LayoutQuery { region: Region::Full, limit: 1 }).unwrap_err(),
            expected
        );
        assert_eq!(
            host.region_features(RegionFeaturesQuery { region: Region::Full, limit: 1 })
                .unwrap_err(),
            expected
        );
        assert_eq!(
            host.template_match(TemplateMatchQuery {
                template_handle: "x".into(),
                region: Region::Full,
                limit: 1,
            })
            .unwrap_err(),
            expected
        );
    }
}
```

Run: `rtk cargo test -p rollshot-vision`
Expected: FAIL to compile because `RealAutomationHost` does not exist.

- [ ] **Step 6: Create `crates/rollshot-vision/src/error.rs`**

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
    #[error("template store limit exceeded: {code}")]
    StoreLimit { code: &'static str },
    #[error("io/serialization failure: {code}")]
    Io { code: &'static str },
}
```

- [ ] **Step 7: Create `crates/rollshot-vision/src/host.rs` (PR1 stub)**

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
        // Replaced by prepared cached lookup in PR4.
        Err(CapabilityError::Failed { code: "capability_unavailable" })
    }
}
```

- [ ] **Step 8: Wire modules and exports in `crates/rollshot-vision/src/lib.rs`**

```rust
//! Rollshot-specific, deterministic, UI-oriented vision adapter layer.
//! Implements the `rollshot_automation::AutomationHost` capability boundary.

#![forbid(unsafe_code)]

mod error;
mod host;

pub use error::VisionError;
pub use host::RealAutomationHost;
```

Keep the `contract_tests` module from Step 5 below these exports.

- [ ] **Step 9: Run the test to verify GREEN**

Run: `rtk cargo test -p rollshot-vision`
Expected: PASS (1 test). The crate compiles and implements `AutomationHost`.

- [ ] **Step 10: Verify workspace-wide build and lints**

Run: `rtk cargo build --workspace`
Expected: success. Run `rtk cargo clippy -p rollshot-vision -- -D warnings`; expected: no warnings.

- [ ] **Step 11: Write the PR1 handoff note**

Create `docs/superpowers/handoffs/2026-06-22-rollshot-vision.md` with:

> PR1 done — `rollshot-vision` crate exists, `RealAutomationHost` implements `AutomationHost` returning `capability_unavailable` for all four capabilities; `imageproc` is pinned in the workspace at 0.26 with default features disabled. Next: PR2 `VisualIndex` + `rect.rs`.

- [ ] **Step 12: Commit**

```bash
rtk git add Cargo.toml crates/rollshot-core/Cargo.toml crates/rollshot-vision docs/superpowers/handoffs/2026-06-22-rollshot-vision.md
rtk git commit -m "feat(vision): add runtime host skeleton" -m "Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 2: `VisualIndex` + `rect.rs` (PR2)

**Files:**
- Create: `crates/rollshot-vision/src/rect.rs`
- Create: `crates/rollshot-vision/src/index.rs`
- Modify: `crates/rollshot-vision/src/lib.rs` (add `mod rect; mod index;` + re-exports)
- Modify: `docs/superpowers/handoffs/2026-06-22-rollshot-vision.md`

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
    fn pixel_rect_rejects_non_finite_endpoints() {
        let e = to_pixel_rect(
            r(f32::MAX, 0.0, f32::MAX, 1.0),
            10,
            10,
            MAX_SEARCH_AREA,
        )
        .unwrap_err();
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
    if !x0.is_finite() || !y0.is_finite() || !x1.is_finite() || !y1.is_finite() {
        return Err(CapabilityError::InvalidInput { code: "non_finite_region" });
    }

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
Expected: PASS (8 tests).

- [ ] **Step 5: Write the failing test for `VisualIndex`**

Create `crates/rollshot-vision/src/index.rs` with a test module first:

```rust
#[cfg(test)]
mod tests {
    use std::path::PathBuf;

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

- [ ] **Step 10: Append the PR2 handoff note**

Append: "PR2 done — `VisualIndex::build` caches grayscale and rejects empty input; rectangle conversion uses floor-min/ceil-max, validates finite endpoints, and enforces empty/area rules. Next: PR3 template store."

- [ ] **Step 11: Commit**

```bash
rtk git add crates/rollshot-vision/src docs/superpowers/handoffs/2026-06-22-rollshot-vision.md
rtk git commit -m "feat(vision): add visual index and pixel geometry" -m "Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 3: `TemplateStore` + sensitivity + serialization gate (PR3)

**Files:**
- Create: `crates/rollshot-vision/src/template.rs`
- Modify: `crates/rollshot-vision/src/lib.rs` (add `mod template;` + re-exports)
- Modify: `docs/superpowers/handoffs/2026-06-22-rollshot-vision.md`

**Interfaces:**
- Consumes: `VisionError`, `rollshot_image_document::ImageRect`.
- Produces:
  - `template::TemplateSensitivity { Chrome, Sensitive }` (derives `Debug, Clone, Copy, PartialEq, Eq`).
  - `template::TemplateBytes` with `new(width: u32, height: u32, rgba: Vec<u8>) -> Result<Self, VisionError>`, `width()`, `height()`, `byte_len()`, `to_rgba_image() -> image::RgbaImage`, and `MAX_TEMPLATE_AREA: u64`.
  - `template::TemplateSource { UserRect, AgentSuggested }` (placeholder provenance enum).
  - `template::TemplateAsset { handle: String, sensitivity, source, created_at_ms: u64, bounds_in_source_image: Option<ImageRect>, bytes: TemplateBytes }` — **no generic `Serialize`**.
  - `template::TemplateStore` with `new()`, bounded `insert(asset) -> Result<(), VisionError>`, `get(handle) -> Option<&TemplateAsset>`, `save_local(path)`, `load_local(path)`, and `export(path)`.
  - `template::LocalTemplateAssetRecord` (bytes present) and `template::ExportTemplateAssetRecord` (`bytes: Option<TemplateBytesRecord>`, `None` for `Sensitive`) — these are the only `Serialize` carriers.
  - `MAX_TEMPLATE_COUNT` and `MAX_TEMPLATE_STORE_BYTES`; replacement inserts account for the replaced asset rather than double-counting it.

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

    fn temp_file(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "rollshot-vision-{}-{name}.json",
            std::process::id()
        ))
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
        store.insert(asset("a", TemplateSensitivity::Chrome)).unwrap();
        assert!(store.get("a").is_some());
        assert!(store.get("missing").is_none());
    }

    #[test]
    fn local_round_trip_keeps_all_bytes_and_export_strips_sensitive() {
        let local_path = temp_file("local-round-trip");
        let export_path = temp_file("export-strip");
        let mut store = TemplateStore::new();
        store.insert(asset("chrome", TemplateSensitivity::Chrome)).unwrap();
        store.insert(asset("secret", TemplateSensitivity::Sensitive)).unwrap();

        store.save_local(&local_path).unwrap();
        let loaded = TemplateStore::load_local(&local_path).unwrap();
        assert_eq!(loaded.get("secret").unwrap().bytes.byte_len(), 4 * 4 * 4);

        store.export(&export_path).unwrap();
        let json = std::fs::read(&export_path).unwrap();
        let exported: Vec<ExportTemplateAssetRecord> = serde_json::from_slice(&json).unwrap();
        let secret = exported.iter().find(|r| r.handle == "secret").unwrap();
        let chrome = exported.iter().find(|r| r.handle == "chrome").unwrap();
        assert!(secret.bytes.is_none(), "sensitive bytes must be stripped on export");
        assert!(chrome.bytes.is_some(), "chrome bytes are kept on export");

        let _ = std::fs::remove_file(local_path);
        let _ = std::fs::remove_file(export_path);
    }

    #[test]
    fn load_rejects_corrupt_records() {
        let path = temp_file("corrupt");
        std::fs::write(&path, br#"[{"handle":"x"}]"#).unwrap();
        assert_eq!(
            TemplateStore::load_local(&path).unwrap_err(),
            VisionError::Io { code: "deserialize" }
        );
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn store_rejects_too_many_templates() {
        let mut store = TemplateStore::with_limits(2, 1024);
        for i in 0..2 {
            store
                .insert(asset(&format!("template-{i}"), TemplateSensitivity::Chrome))
                .unwrap();
        }
        assert_eq!(
            store
                .insert(asset("one-too-many", TemplateSensitivity::Chrome))
                .unwrap_err(),
            VisionError::StoreLimit { code: "too_many_templates" }
        );
    }

    #[test]
    fn store_byte_limit_accounts_for_replacement() {
        let mut store = TemplateStore::with_limits(4, 64);
        store.insert(asset("same", TemplateSensitivity::Chrome)).unwrap();
        store.insert(asset("same", TemplateSensitivity::Sensitive)).unwrap();
        assert_eq!(store.total_bytes, 64);
        assert_eq!(
            store
                .insert(asset("overflow", TemplateSensitivity::Chrome))
                .unwrap_err(),
            VisionError::StoreLimit { code: "store_too_large" }
        );
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
use std::path::Path;

use rollshot_image_document::ImageRect;
use serde::{Deserialize, Serialize};

use crate::VisionError;

/// Cap on a single template's pixel area.
pub const MAX_TEMPLATE_AREA: u64 = 1_048_576; // 1024x1024
/// Cap on templates in one preset-local store.
pub const MAX_TEMPLATE_COUNT: usize = 256;
/// Cap on raw RGBA bytes retained by one store.
pub const MAX_TEMPLATE_STORE_BYTES: usize = 64 * 1024 * 1024;

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

    pub fn byte_len(&self) -> usize {
        self.rgba.len()
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

#[derive(Debug)]
pub struct TemplateStore {
    assets: BTreeMap<String, TemplateAsset>,
    total_bytes: usize,
    max_count: usize,
    max_bytes: usize,
}

impl Default for TemplateStore {
    fn default() -> Self {
        Self::new()
    }
}

impl TemplateStore {
    pub fn new() -> Self {
        Self {
            assets: BTreeMap::new(),
            total_bytes: 0,
            max_count: MAX_TEMPLATE_COUNT,
            max_bytes: MAX_TEMPLATE_STORE_BYTES,
        }
    }

    #[cfg(test)]
    fn with_limits(max_count: usize, max_bytes: usize) -> Self {
        Self {
            assets: BTreeMap::new(),
            total_bytes: 0,
            max_count,
            max_bytes,
        }
    }

    pub fn insert(&mut self, asset: TemplateAsset) -> Result<(), VisionError> {
        let replaced_len = self
            .assets
            .get(&asset.handle)
            .map(|old| old.bytes.byte_len())
            .unwrap_or(0);
        let is_new = !self.assets.contains_key(&asset.handle);
        if is_new && self.assets.len() >= self.max_count {
            return Err(VisionError::StoreLimit { code: "too_many_templates" });
        }
        let next_total = self
            .total_bytes
            .checked_sub(replaced_len)
            .and_then(|n| n.checked_add(asset.bytes.byte_len()))
            .ok_or(VisionError::StoreLimit { code: "template_bytes_overflow" })?;
        if next_total > self.max_bytes {
            return Err(VisionError::StoreLimit { code: "store_too_large" });
        }
        self.assets.insert(asset.handle.clone(), asset);
        self.total_bytes = next_total;
        Ok(())
    }

    pub fn get(&self, handle: &str) -> Option<&TemplateAsset> {
        self.assets.get(handle)
    }

    /// Local persistence: keeps all bytes (chrome + sensitive).
    pub fn save_local(&self, dst: &Path) -> Result<(), VisionError> {
        let records: Vec<_> = self
            .assets
            .values()
            .map(LocalTemplateAssetRecord::from_asset)
            .collect();
        let bytes =
            serde_json::to_vec(&records).map_err(|_| VisionError::Io { code: "serialize" })?;
        std::fs::write(dst, bytes).map_err(|_| VisionError::Io { code: "write" })
    }

    pub fn load_local(src: &Path) -> Result<Self, VisionError> {
        let bytes = std::fs::read(src).map_err(|_| VisionError::Io { code: "read" })?;
        let records: Vec<LocalTemplateAssetRecord> =
            serde_json::from_slice(&bytes).map_err(|_| VisionError::Io { code: "deserialize" })?;
        let mut store = Self::new();
        for record in records {
            store.insert(record.into_asset()?)?;
        }
        Ok(store)
    }

    /// Export: strips `Sensitive` bytes before any serialization occurs.
    pub fn export(&self, dst: &Path) -> Result<(), VisionError> {
        let records: Vec<_> = self
            .assets
            .values()
            .map(ExportTemplateAssetRecord::from_asset)
            .collect();
        let bytes =
            serde_json::to_vec(&records).map_err(|_| VisionError::Io { code: "serialize" })?;
        std::fs::write(dst, bytes).map_err(|_| VisionError::Io { code: "write" })
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

    fn into_asset(self) -> Result<TemplateAsset, VisionError> {
        Ok(TemplateAsset {
            handle: self.handle,
            sensitivity: if self.sensitivity_sensitive {
                TemplateSensitivity::Sensitive
            } else {
                TemplateSensitivity::Chrome
            },
            source: if self.source_agent_suggested {
                TemplateSource::AgentSuggested
            } else {
                TemplateSource::UserRect
            },
            created_at_ms: self.created_at_ms,
            bounds_in_source_image: self.bounds_in_source_image,
            bytes: TemplateBytes::new(self.width, self.height, self.bytes.rgba)?,
        })
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
    static_assertions::assert_not_impl_any!(TemplateAsset: serde::Serialize);
    static_assertions::assert_not_impl_any!(TemplateStore: serde::Serialize);
    static_assertions::assert_not_impl_any!(TemplateBytes: serde::Serialize);
    static_assertions::assert_impl_all!(LocalTemplateAssetRecord: serde::Serialize);
    static_assertions::assert_impl_all!(ExportTemplateAssetRecord: serde::Serialize);
```

These assertions fail compilation if a future change adds a generic serialization escape hatch.

- [ ] **Step 5: Wire `template` into `lib.rs`**

```rust
mod template;

pub use template::{
    ExportTemplateAssetRecord, LocalTemplateAssetRecord, TemplateAsset, TemplateBytes,
    TemplateBytesRecord, TemplateSensitivity, TemplateSource, TemplateStore, MAX_TEMPLATE_AREA,
    MAX_TEMPLATE_COUNT, MAX_TEMPLATE_STORE_BYTES,
};
```

- [ ] **Step 6: Run template tests**

Run: `rtk cargo test -p rollshot-vision template`
Expected: PASS (8 runtime tests plus compile-time serialization assertions).

- [ ] **Step 7: Append the PR3 handoff note**

Append: "PR3 done — checked raw-RGBA templates; bounded in-memory store; explicit local save/load and export records; corrupt local data is rejected; export strips `Sensitive` bytes; asset/store types have compile-time no-Serialize assertions. Next: PR4 prepared template matching."

- [ ] **Step 8: Commit**

```bash
rtk git add crates/rollshot-vision docs/superpowers/handoffs/2026-06-22-rollshot-vision.md
rtk git commit -m "feat(vision): add bounded private template store" -m "Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 4: `templateMatch` v0 + NMS (PR4)

**Files:**
- Modify: `crates/rollshot-vision/src/template.rs` (add bounded `match_template_image`, peak extraction, NMS)
- Modify: `crates/rollshot-vision/src/host.rs` (prepared query cache; no detector work in callbacks)
- Modify: `crates/rollshot-vision/src/lib.rs` (re-exports if needed)
- Modify: `docs/superpowers/handoffs/2026-06-22-rollshot-vision.md`

**Interfaces:**
- Consumes: `VisualIndex::gray()`, `rect::{region_to_pixel_rect, iou, MAX_SEARCH_AREA, PixelRect}`, `TemplateStore::get`, `rollshot_automation::{TemplateMatchQuery, TemplateMatch, CapabilityError, Region}`, `rollshot_image_document::{ImageRect, ImagePoint}`, imageproc template matching.
- Produces:
  - `template::match_template_image(index: &VisualIndex, tpl_gray: &image::GrayImage, region: &Region, limit: u32) -> Result<Vec<TemplateMatch>, CapabilityError>`.
  - `template::prepare_template_match(index: &VisualIndex, store: &TemplateStore, q: &TemplateMatchQuery) -> Result<Vec<TemplateMatch>, CapabilityError>`.
  - `RealAutomationHost::new() -> Self` and `prepare_template_match(&mut self, index, store, query) -> Result<(), CapabilityError>`.
  - Callback behavior: only prepared handle+region queries are accepted; request `limit` may be at most the prepared limit; an unprepared query returns `Failed { code: "vision_index_unavailable" }`.
  - `MAX_TEMPLATE_MATCH_PIXEL_VISITS` and `MAX_SCORE_POSITIONS` checked before `imageproc` allocation/work.

- [ ] **Step 1: Confirm the imageproc 0.26 template-matching API**

Run: `rtk cargo doc -p imageproc --no-deps` is unnecessary; instead grep the dependency source:

Run: `rtk cargo tree -p rollshot-core -i imageproc` to record the resolved patch release, then locate that exact source under `~/.cargo/registry/src` and read `template_matching.rs`.
Expected for the currently resolved 0.26.2 API: `match_template(&GrayImage, &GrayImage, MatchTemplateMethod) -> Image<Luma<f32>>`; `CrossCorrelationNormalized` is normalized dot-product correlation rather than zero-mean NCC; and the function panics unless both template dimensions are strictly smaller than the image. Use `CrossCorrelation` for the raw dot-product map, normalize it with scene/template means and variances, and keep the equal-dimension raw-dot fallback below.

- [ ] **Step 2: Write the failing test (find a pasted template + NMS + limit + errors)**

Add to the `tests` module in `crates/rollshot-vision/src/template.rs`:

```rust
    use crate::index::VisualIndex;
    use rollshot_automation::{CapabilityError, Region, TemplateMatchQuery};

    /// 40x40 deterministic textured scene with a non-periodic 8x8 glyph pasted
    /// at (10,12) and (28,6). Returns (scene, template_bytes).
    fn scene_with_two_marks() -> (image::RgbaImage, TemplateBytes) {
        let mut scene = image::RgbaImage::from_fn(40, 40, |x, y| {
            let v = 120 + ((x * 3 + y * 5) % 23) as u8;
            image::Rgba([v, v, v, 255])
        });
        for &(ox, oy) in &[(10u32, 12u32), (28, 6)] {
            for dy in 0..8 {
                for dx in 0..8 {
                    let v = ((dx * 31 + dy * 17 + dx * dy * 7) % 220) as u8;
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
        })
        .unwrap();
        store
    }

    #[test]
    fn finds_both_instances_with_nms() {
        let (scene, tpl) = scene_with_two_marks();
        let index = VisualIndex::build(scene).unwrap();
        let store = store_with("mark", tpl, TemplateSensitivity::Chrome);
        let matches = prepare_template_match(
            &index,
            &store,
            &TemplateMatchQuery {
                template_handle: "mark".into(),
                region: Region::Full,
                limit: 2,
            },
        )
        .unwrap();
        // The host has no confidence threshold, so request exactly the two
        // expected peaks rather than asserting that every lower peak vanishes.
        assert_eq!(matches.len(), 2);
        assert!(matches.iter().all(|m| m.score > 0.99));
        let positions: std::collections::BTreeSet<_> = matches
            .iter()
            .map(|m| (m.bounds.x as i32, m.bounds.y as i32))
            .collect();
        assert_eq!(positions, [(10, 12), (28, 6)].into_iter().collect());
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
        let matches = prepare_template_match(
            &index,
            &store,
            &TemplateMatchQuery {
                template_handle: "mark".into(),
                region: Region::Full,
                limit: 1,
            },
        )
        .unwrap();
        assert_eq!(matches.len(), 1);
    }

    #[test]
    fn missing_handle_is_typed_error() {
        let (scene, _tpl) = scene_with_two_marks();
        let index = VisualIndex::build(scene).unwrap();
        let store = TemplateStore::new();
        let e = prepare_template_match(
            &index,
            &store,
            &TemplateMatchQuery {
                template_handle: "nope".into(),
                region: Region::Full,
                limit: 10,
            },
        )
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
        let e = prepare_template_match(
            &index,
            &store,
            &TemplateMatchQuery {
                template_handle: "flat".into(),
                region: Region::Full,
                limit: 10,
            },
        )
        .unwrap_err();
        assert_eq!(e, CapabilityError::InvalidInput { code: "template_low_information" });
    }

    #[test]
    fn template_larger_than_region_is_error() {
        let scene = image::RgbaImage::from_pixel(6, 6, image::Rgba([180, 180, 180, 255]));
        let index = VisualIndex::build(scene).unwrap();
        let mut big_rgba = vec![0u8; 8 * 8 * 4];
        for i in 0..(8 * 8) {
            let v = ((i * 37) % 251) as u8;
            big_rgba[i * 4..i * 4 + 4].copy_from_slice(&[v, v, v, 255]);
        }
        let big = TemplateBytes::new(8, 8, big_rgba).unwrap();
        let store = store_with("big", big, TemplateSensitivity::Chrome);
        let e = prepare_template_match(
            &index,
            &store,
            &TemplateMatchQuery {
                template_handle: "big".into(),
                region: Region::Full,
                limit: 10,
            },
        )
        .unwrap_err();
        assert_eq!(e, CapabilityError::InvalidInput { code: "template_larger_than_region" });
    }

    #[test]
    fn zero_limit_is_rejected_by_core_api() {
        let (scene, tpl) = scene_with_two_marks();
        let index = VisualIndex::build(scene).unwrap();
        let store = store_with("mark", tpl, TemplateSensitivity::Chrome);
        let e = prepare_template_match(
            &index,
            &store,
            &TemplateMatchQuery {
                template_handle: "mark".into(),
                region: Region::Full,
                limit: 0,
            },
        )
        .unwrap_err();
        assert_eq!(e, CapabilityError::InvalidInput { code: "invalid_query" });
    }

    #[test]
    fn template_equal_to_region_scores_one_position_without_panicking() {
        let (scene, tpl) = scene_with_two_marks();
        let exact = image::imageops::crop_imm(&scene, 10, 12, 8, 8).to_image();
        let index = VisualIndex::build(exact).unwrap();
        let store = store_with("exact", tpl, TemplateSensitivity::Chrome);
        let matches = prepare_template_match(
            &index,
            &store,
            &TemplateMatchQuery {
                template_handle: "exact".into(),
                region: Region::Full,
                limit: 1,
            },
        )
        .unwrap();
        assert_eq!(matches.len(), 1);
        assert!(matches[0].score > 0.99);
    }

    #[test]
    fn excessive_match_work_is_rejected_before_matching() {
        let scene = image::RgbaImage::from_pixel(1000, 1000, image::Rgba([80, 80, 80, 255]));
        let index = VisualIndex::build(scene).unwrap();
        let tpl_image = image::RgbaImage::from_fn(64, 64, |x, y| {
            let v = ((x * 31 + y * 17 + x * y * 7) % 251) as u8;
            image::Rgba([v, v, v, 255])
        });
        let tpl = TemplateBytes::new(64, 64, tpl_image.into_raw()).unwrap();
        let store = store_with("mark", tpl, TemplateSensitivity::Chrome);
        let e = prepare_template_match(
            &index,
            &store,
            &TemplateMatchQuery {
                template_handle: "mark".into(),
                region: Region::Full,
                limit: 1,
            },
        )
        .unwrap_err();
        assert_eq!(e, CapabilityError::InvalidInput { code: "region_too_large" });
    }
```

- [ ] **Step 3: Implement bounded matching, candidate extraction, and NMS**

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
/// Maximum score-map cells allocated by one prepared query. At this ceiling,
/// the f32 score map is ~16 MiB; two f64 integral-moment planes are ~64 MiB
/// for a similarly sized search image, before source/crop buffers.
pub const MAX_SCORE_POSITIONS: u64 = 4_000_000;
/// Maximum sliding-window pixel visits for one prepared query.
pub const MAX_TEMPLATE_MATCH_PIXEL_VISITS: u64 = 250_000_000;
/// Oversampling before NMS so one strong cluster does not hide later instances.
const PEAK_OVERSAMPLE: u32 = 64;

fn gray_variance(gray: &image::GrayImage) -> f32 {
    let n = f64::from(gray.width()) * f64::from(gray.height());
    if n == 0.0 {
        return 0.0;
    }
    let mut sum = 0.0f64;
    let mut sum_sq = 0.0f64;
    for p in gray.pixels() {
        let v = f64::from(p.0[0]);
        sum += v;
        sum_sq += v * v;
    }
    let mean = sum / n;
    ((sum_sq / n) - mean * mean) as f32
}

pub(crate) fn prepare_template_match(
    index: &VisualIndex,
    store: &TemplateStore,
    q: &TemplateMatchQuery,
) -> Result<Vec<TemplateMatch>, CapabilityError> {
    if q.limit == 0 {
        return Err(CapabilityError::InvalidInput { code: "invalid_query" });
    }
    let asset = store
        .get(&q.template_handle)
        .ok_or(CapabilityError::Failed { code: "template_not_found" })?;
    let tpl_gray = image::imageops::grayscale(&asset.bytes.to_rgba_image());
    match_template_image(index, &tpl_gray, &q.region, q.limit)
}

/// Core NCC + NMS matcher shared by the capability and self-validation. Takes a
/// grayscale template directly (no store handle).
pub(crate) fn match_template_image(
    index: &VisualIndex,
    tpl_gray: &image::GrayImage,
    region: &Region,
    limit: u32,
) -> Result<Vec<TemplateMatch>, CapabilityError> {
    if limit == 0 {
        return Err(CapabilityError::InvalidInput { code: "invalid_query" });
    }
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
    let positions = u64::from(search.width - tw + 1)
        .checked_mul(u64::from(search.height - th + 1))
        .ok_or(CapabilityError::InvalidInput { code: "region_too_large" })?;
    let template_area = u64::from(tw)
        .checked_mul(u64::from(th))
        .ok_or(CapabilityError::InvalidInput { code: "region_too_large" })?;
    let pixel_visits = positions
        .checked_mul(template_area)
        .ok_or(CapabilityError::InvalidInput { code: "region_too_large" })?;
    if positions > MAX_SCORE_POSITIONS || pixel_visits > MAX_TEMPLATE_MATCH_PIXEL_VISITS {
        return Err(CapabilityError::InvalidInput { code: "region_too_large" });
    }

    // Crop the scene grayscale to the search region.
    let scene = image::imageops::crop_imm(index.gray(), search.x, search.y, search.width, search.height)
        .to_image();

    // imageproc panics when either template dimension equals the scene
    // dimension. Handle that valid edge shape with direct raw dot products.
    let raw_map: image::ImageBuffer<Luma<f32>, Vec<f32>> =
        if scene.width() == tw || scene.height() == th {
            match_equal_dimension(&scene, tpl_gray)
        } else {
            match_template(&scene, tpl_gray, MatchTemplateMethod::CrossCorrelation)
        };
    let score_map = zero_mean_normalize(&scene, tpl_gray, raw_map);

    // Keep only a bounded top-K candidate pool before NMS. Never materialize
    // one heap object per score-map cell.
    let candidate_cap = limit
        .saturating_mul(PEAK_OVERSAMPLE)
        .clamp(64, 8_192) as usize;
    let mut candidates = std::collections::BinaryHeap::<
        std::cmp::Reverse<Peak>,
    >::with_capacity(candidate_cap);
    for (mx, my, px) in score_map.enumerate_pixels() {
        let score = px.0[0];
        if !score.is_finite() {
            continue;
        }
        let peak = Peak { score, x: search.x + mx, y: search.y + my };
        if candidates.len() < candidate_cap {
            candidates.push(std::cmp::Reverse(peak));
        } else if candidates.peek().is_some_and(|worst| peak > worst.0) {
            candidates.pop();
            candidates.push(std::cmp::Reverse(peak));
        }
    }

    let mut candidates: Vec<_> = candidates.into_iter().map(|p| p.0).collect();
    candidates.sort_by(|a, b| b.cmp(a));

    // Greedy NMS.
    let mut kept: Vec<(f32, ImageRect)> = Vec::new();
    for peak in candidates {
        let score = peak.score;
        let rect = ImageRect {
            x: peak.x as f32,
            y: peak.y as f32,
            width: tw as f32,
            height: th as f32,
        };
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

#[derive(Debug, Clone, Copy, PartialEq)]
struct Peak {
    score: f32,
    x: u32,
    y: u32,
}

impl Eq for Peak {}

impl Ord for Peak {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.score
            .total_cmp(&other.score)
            // For equal scores, smaller x/y rank higher after descending sort.
            .then_with(|| other.x.cmp(&self.x))
            .then_with(|| other.y.cmp(&self.y))
    }
}

impl PartialOrd for Peak {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

fn match_equal_dimension(
    scene: &image::GrayImage,
    template: &image::GrayImage,
) -> image::ImageBuffer<Luma<f32>, Vec<f32>> {
    let out_w = scene.width() - template.width() + 1;
    let out_h = scene.height() - template.height() + 1;
    image::ImageBuffer::from_fn(out_w, out_h, |x, y| {
        Luma([dot_at(scene, template, x, y) as f32])
    })
}

fn dot_at(
    scene: &image::GrayImage,
    template: &image::GrayImage,
    offset_x: u32,
    offset_y: u32,
) -> f64 {
    let mut dot = 0.0f64;
    for y in 0..template.height() {
        for x in 0..template.width() {
            let s = f64::from(scene.get_pixel(offset_x + x, offset_y + y).0[0]);
            let t = f64::from(template.get_pixel(x, y).0[0]);
            dot += s * t;
        }
    }
    dot
}

fn zero_mean_normalize(
    scene: &image::GrayImage,
    template: &image::GrayImage,
    raw_map: image::ImageBuffer<Luma<f32>, Vec<f32>>,
) -> image::ImageBuffer<Luma<f32>, Vec<f32>> {
    let moments = IntegralMoments::build(scene);
    let n = f64::from(template.width()) * f64::from(template.height());
    let template_sum: f64 = template.pixels().map(|p| f64::from(p.0[0])).sum();
    let template_sq: f64 = template
        .pixels()
        .map(|p| {
            let v = f64::from(p.0[0]);
            v * v
        })
        .sum();
    let template_var = template_sq - template_sum * template_sum / n;

    image::ImageBuffer::from_fn(raw_map.width(), raw_map.height(), |x, y| {
        let (scene_sum, scene_sq) =
            moments.rect(x, y, template.width(), template.height());
        let scene_var = scene_sq - scene_sum * scene_sum / n;
        let numerator =
            f64::from(raw_map.get_pixel(x, y).0[0]) - scene_sum * template_sum / n;
        let score = if scene_var > 1.0 && template_var > 1.0 {
            (numerator / (scene_var * template_var).sqrt()) as f32
        } else {
            f32::NAN
        };
        Luma([score])
    })
}

struct IntegralMoments {
    width: usize,
    sum: Vec<f64>,
    square_sum: Vec<f64>,
}

impl IntegralMoments {
    fn build(image: &image::GrayImage) -> Self {
        let width = image.width() as usize + 1;
        let height = image.height() as usize + 1;
        let mut sum = vec![0.0; width * height];
        let mut square_sum = vec![0.0; width * height];
        for y in 0..image.height() as usize {
            let mut row_sum = 0.0;
            let mut row_square_sum = 0.0;
            for x in 0..image.width() as usize {
                let v = f64::from(image.get_pixel(x as u32, y as u32).0[0]);
                row_sum += v;
                row_square_sum += v * v;
                let index = (y + 1) * width + x + 1;
                sum[index] = sum[y * width + x + 1] + row_sum;
                square_sum[index] = square_sum[y * width + x + 1] + row_square_sum;
            }
        }
        Self { width, sum, square_sum }
    }

    fn rect(&self, x: u32, y: u32, width: u32, height: u32) -> (f64, f64) {
        let x0 = x as usize;
        let y0 = y as usize;
        let x1 = x0 + width as usize;
        let y1 = y0 + height as usize;
        let read = |values: &[f64]| {
            values[y1 * self.width + x1]
                - values[y0 * self.width + x1]
                - values[y1 * self.width + x0]
                + values[y0 * self.width + x0]
        };
        (read(&self.sum), read(&self.square_sum))
    }
}
```

- [ ] **Step 4: Run the templateMatch tests**

Run: `rtk cargo test -p rollshot-vision template`
Expected: PASS (8 matching tests + the PR3 store tests). The matcher returns up to the requested limit without a host-side confidence threshold; tests assert ranked locations and typed errors, not that every low-score position disappears.

- [ ] **Step 5: Wire `RealAutomationHost` to prepared results**

Replace `crates/rollshot-vision/src/host.rs` struct/impl with:

```rust
use std::time::Instant;

use rollshot_automation::{
    AutomationHost, CapabilityError, LayoutQuery, LayoutRegion, OcrMatch, OcrQuery, RegionFeatures,
    RegionFeaturesQuery, TemplateMatch, TemplateMatchQuery,
};

use crate::index::VisualIndex;
use crate::template::{prepare_template_match as prepare_template_results, TemplateStore};

#[derive(Debug, Clone)]
struct PreparedTemplateMatch {
    template_handle: String,
    region: rollshot_automation::Region,
    max_limit: u32,
    results: Vec<TemplateMatch>,
}

#[derive(Debug, Default)]
pub struct RealAutomationHost {
    prepared_template_matches: Vec<PreparedTemplateMatch>,
}

impl RealAutomationHost {
    pub fn new() -> Self {
        Self::default()
    }

    /// Expensive preparation. Call before entering `QuickJsExecutor`.
    pub fn prepare_template_match(
        &mut self,
        index: &VisualIndex,
        templates: &TemplateStore,
        query: &TemplateMatchQuery,
    ) -> Result<(), CapabilityError> {
        let started = Instant::now();
        let results = prepare_template_results(index, templates, query)?;
        self.prepared_template_matches.retain(|prepared| {
            prepared.template_handle != query.template_handle || prepared.region != query.region
        });
        self.prepared_template_matches.push(PreparedTemplateMatch {
            template_handle: query.template_handle.clone(),
            region: query.region,
            max_limit: query.limit,
            results,
        });
        tracing::debug!(
            target: "rollshot::vision::template",
            duration_ms = started.elapsed().as_millis() as u64,
            result_count = self
                .prepared_template_matches
                .last()
                .map_or(0, |prepared| prepared.results.len()),
            "template query prepared"
        );
        Ok(())
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
        if query.limit == 0 {
            return Err(CapabilityError::InvalidInput { code: "invalid_query" });
        }
        let prepared = self
            .prepared_template_matches
            .iter()
            .find(|prepared| {
                prepared.template_handle == query.template_handle && prepared.region == query.region
            })
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
}
```

Update the PR1 contract test so it continues to assert `ocr`/`layout`/`region_features`, but removes the old `template_match` assertion. Add host tests for prepared lookup and the unprepared failure:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::index::VisualIndex;
    use crate::template::{
        TemplateAsset, TemplateBytes, TemplateSensitivity, TemplateSource, TemplateStore,
    };
    use rollshot_automation::Region;

    #[test]
    fn unprepared_template_query_fails_explicitly() {
        let mut host = RealAutomationHost::new();
        let err = host
            .template_match(TemplateMatchQuery {
                template_handle: "missing-preparation".into(),
                region: Region::Full,
                limit: 1,
            })
            .unwrap_err();
        assert_eq!(err, CapabilityError::Failed { code: "vision_index_unavailable" });
    }

    #[test]
    fn prepared_callback_only_looks_up_and_truncates() {
        let mut scene = image::RgbaImage::from_pixel(16, 16, image::Rgba([120, 120, 120, 255]));
        for y in 0..4 {
            for x in 0..4 {
                let v = (x * 47 + y * 29) as u8;
                scene.put_pixel(6 + x, 7 + y, image::Rgba([v, v, v, 255]));
            }
        }
        let tpl = image::imageops::crop_imm(&scene, 6, 7, 4, 4).to_image();
        let index = VisualIndex::build(scene).unwrap();
        let mut store = TemplateStore::new();
        store
            .insert(TemplateAsset {
                handle: "mark".into(),
                sensitivity: TemplateSensitivity::Chrome,
                source: TemplateSource::UserRect,
                created_at_ms: 0,
                bounds_in_source_image: None,
                bytes: TemplateBytes::new(4, 4, tpl.into_raw()).unwrap(),
            })
            .unwrap();
        let prepared_query = TemplateMatchQuery {
            template_handle: "mark".into(),
            region: Region::Full,
            limit: 4,
        };
        let mut host = RealAutomationHost::new();
        host.prepare_template_match(&index, &store, &prepared_query).unwrap();

        let results = host
            .template_match(TemplateMatchQuery {
                limit: 1,
                ..prepared_query.clone()
            })
            .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(
            host.template_match(TemplateMatchQuery {
                limit: 5,
                ..prepared_query
            })
            .unwrap_err(),
            CapabilityError::LimitExceeded
        );
    }
}
```

- [ ] **Step 6: Wire exports and verify module visibility**

In `lib.rs`, keep `index` private and publicly re-export `VisualIndex`, `RealAutomationHost`, and the documented constants/types. `prepare_template_match` remains crate-private; callers prepare through the host method so the callback contract cannot be bypassed accidentally.

```rust
pub use template::{MAX_SCORE_POSITIONS, MAX_TEMPLATE_MATCH_PIXEL_VISITS};
```

- [ ] **Step 7: Run all crate tests + clippy**

Run: `rtk cargo test -p rollshot-vision`
Expected: PASS.
Run: `rtk cargo clippy -p rollshot-vision -- -D warnings`
Expected: clean.

- [ ] **Step 8: Append the PR4 handoff note**

Append: "PR4 done — template work is prepared outside QuickJS; callbacks only perform cached lookup/truncation. Matching validates low-information templates, equal-dimension imageproc edge cases, score finiteness, score-position/pixel-visit budgets, bounded candidate extraction, deterministic ordering, and NMS. Next: PR5 self-validation."

- [ ] **Step 9: Commit**

```bash
rtk git add crates/rollshot-vision docs/superpowers/handoffs/2026-06-22-rollshot-vision.md
rtk git commit -m "feat(vision): add prepared template matching" -m "Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 5: `TemplateSelfValidation` (PR5)

**Files:**
- Create: `crates/rollshot-vision/src/self_validation.rs`
- Modify: `crates/rollshot-vision/src/lib.rs` (add `mod self_validation;` + re-exports)
- Modify: `docs/superpowers/handoffs/2026-06-22-rollshot-vision.md`

**Interfaces:**
- Consumes: `VisualIndex` (`image()`, `width()`, `height()`), `template::match_template_image`, `rect::iou`, `rollshot_image_document::ImageRect`, `VisionError`.
- Produces:
  - `self_validation::ExpectedCount { Unique, Repeating, AtLeast(u32) }`.
  - `self_validation::TemplateDecision { Pass, NeedsConfirm, Reject }`.
  - `self_validation::SelfValidationConfig { expected_count: ExpectedCount, target_bounds: Option<ImageRect> }`.
  - `self_validation::TemplateSelfValidation { self_score, second_best_score, peak_margin, false_positive_count, edge_density, entropy, stable_under_jitter, decision }`.
  - `self_validation::self_validate(index: &VisualIndex, candidate_bounds: ImageRect, cfg: &SelfValidationConfig) -> Result<TemplateSelfValidation, VisionError>`.
  - Explicit candidate-area gates and multi-variant jitter checks (±1 px crop/padding where valid plus ±5% brightness); the best self match must overlap the original candidate location.

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

    // Scene with one distinctive non-periodic glyph at (10,12), size 8x8.
    fn distinctive_scene() -> image::RgbaImage {
        let mut scene = image::RgbaImage::from_fn(40, 40, |x, y| {
            let v = 120 + ((x * 3 + y * 5) % 23) as u8;
            image::Rgba([v, v, v, 255])
        });
        for dy in 0..8 {
            for dx in 0..8 {
                let v = ((dx * 31 + dy * 17 + dx * dy * 7) % 220) as u8;
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
        assert!(v.stable_under_jitter);
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

    #[test]
    fn repeating_pattern_rejects_unique_expectation() {
        let mut scene = distinctive_scene();
        let glyph = image::imageops::crop_imm(&scene, 10, 12, 8, 8).to_image();
        image::imageops::replace(&mut scene, &glyph, 26, 12);
        let index = VisualIndex::build(scene).unwrap();
        let v = self_validate(
            &index,
            ImageRect { x: 10.0, y: 12.0, width: 8.0, height: 8.0 },
            &cfg(ExpectedCount::Unique),
        )
        .unwrap();
        assert_eq!(v.decision, TemplateDecision::Reject);
        assert!(v.false_positive_count >= 1);
    }

    #[test]
    fn candidate_area_gate_rejects_tiny_crop() {
        let index = VisualIndex::build(distinctive_scene()).unwrap();
        let v = self_validate(
            &index,
            ImageRect { x: 10.0, y: 12.0, width: 1.0, height: 1.0 },
            &cfg(ExpectedCount::Unique),
        )
        .unwrap();
        assert_eq!(v.decision, TemplateDecision::Reject);
    }

    #[test]
    fn target_coverage_miss_needs_confirmation() {
        let index = VisualIndex::build(distinctive_scene()).unwrap();
        let v = self_validate(
            &index,
            ImageRect { x: 10.0, y: 12.0, width: 8.0, height: 8.0 },
            &SelfValidationConfig {
                expected_count: ExpectedCount::Unique,
                target_bounds: Some(ImageRect {
                    x: 30.0,
                    y: 30.0,
                    width: 5.0,
                    height: 5.0,
                }),
            },
        )
        .unwrap();
        assert_eq!(v.decision, TemplateDecision::NeedsConfirm);
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
use crate::template::{match_template_image, MAX_TEMPLATE_AREA};
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
const MIN_SELF_VALIDATION_AREA: u64 = 16;

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
    let candidate_area = u64::from(cw) * u64::from(ch);
    let area_ok =
        (MIN_SELF_VALIDATION_AREA..=MAX_TEMPLATE_AREA).contains(&candidate_area);

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

    let self_match_index = matches
        .iter()
        .enumerate()
        .filter(|(_, m)| iou(m.bounds, candidate_bounds) >= 0.5)
        .max_by(|(_, a), (_, b)| a.score.total_cmp(&b.score))
        .map(|(index, _)| index);
    let self_score = self_match_index
        .and_then(|index| matches.get(index))
        .map(|m| m.score)
        .unwrap_or(0.0);
    let second_best_score = matches
        .iter()
        .enumerate()
        .filter(|(index, _)| Some(*index) != self_match_index)
        .map(|(_, m)| m.score)
        .max_by(f32::total_cmp);

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
            matches.iter().filter(|m| m.score >= FALSE_POSITIVE_SCORE).count()
                >= n.max(1) as usize
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
        area_ok,
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
    area_ok: bool,
    count_ok: bool,
    coverage_ok: bool,
) -> TemplateDecision {
    let structural_floor = edge_density >= EDGE_DENSITY_FLOOR && entropy >= ENTROPY_FLOOR;
    if self_score < SELF_SCORE_FLOOR
        || !structural_floor
        || false_positive_count > 0
        || !stable
        || !area_ok
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

/// Re-match brightness and ±1 px crop/padding variants. Every available
/// variant must return near its expected source location with bounded score
/// loss; this is author-time validation, so conservative rejection is correct.
fn jitter_stable(
    index: &VisualIndex,
    candidate_rgba: &image::RgbaImage,
    candidate_bounds: ImageRect,
    base_score: f32,
) -> bool {
    let mut variants: Vec<(image::GrayImage, ImageRect)> = Vec::new();

    let mut jittered = candidate_rgba.clone();
    for p in jittered.pixels_mut() {
        for c in 0..3 {
            p.0[c] = ((p.0[c] as f32) * 1.05).min(255.0) as u8;
        }
    }
    variants.push((image::imageops::grayscale(&jittered), candidate_bounds));

    let x = candidate_bounds.x.floor() as u32;
    let y = candidate_bounds.y.floor() as u32;
    let w = candidate_bounds.width.round() as u32;
    let h = candidate_bounds.height.round() as u32;
    if w > 4 && h > 4 {
        let inward = image::imageops::crop_imm(index.image(), x + 1, y + 1, w - 2, h - 2)
            .to_image();
        variants.push((
            image::imageops::grayscale(&inward),
            ImageRect {
                x: (x + 1) as f32,
                y: (y + 1) as f32,
                width: (w - 2) as f32,
                height: (h - 2) as f32,
            },
        ));
    }
    if x > 0 && y > 0 && x + w < index.width() && y + h < index.height() {
        let outward =
            image::imageops::crop_imm(index.image(), x - 1, y - 1, w + 2, h + 2).to_image();
        variants.push((
            image::imageops::grayscale(&outward),
            ImageRect {
                x: (x - 1) as f32,
                y: (y - 1) as f32,
                width: (w + 2) as f32,
                height: (h + 2) as f32,
            },
        ));
    }

    variants.into_iter().all(|(gray, expected)| {
        let matches = match match_template_image(index, &gray, &Region::Full, 4) {
            Ok(matches) => matches,
            Err(_) => return false,
        };
        matches.iter().any(|m| {
            iou(m.bounds, expected) >= 0.5 && base_score - m.score <= JITTER_SCORE_DROP
        })
    })
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
Expected: PASS (6 tests). The positive fixture has enough intensity diversity to clear the entropy floor. Because the matcher intentionally returns ranked low-score peaks up to `limit`, do not assume a one-instance scene yields only one raw match; validate source-location overlap, margin, and false-positive thresholds explicitly.

- [ ] **Step 6: Run all crate tests + clippy**

Run: `rtk cargo test -p rollshot-vision && rtk cargo clippy -p rollshot-vision -- -D warnings`
Expected: PASS, clean.

- [ ] **Step 7: Append the PR5 handoff note**

Append: "PR5 done — self-validation verifies source-location overlap, expected-count behavior, area/target-coverage gates, edge/entropy, false positives, and brightness plus ±1 px crop/padding stability. Next: PR6 integration matrix."

- [ ] **Step 8: Commit**

```bash
rtk git add crates/rollshot-vision docs/superpowers/handoffs/2026-06-22-rollshot-vision.md
rtk git commit -m "feat(vision): add template self-validation" -m "Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 6: role-free QuickJS fixture integration tests (PR6)

**Files:**
- Create: `crates/rollshot-vision/tests/fixtures/hide_bookmarks.js`
- Create: `crates/rollshot-vision/tests/fixtures/hide_folders.js`
- Create: `crates/rollshot-vision/tests/integration.rs`
- Modify: `docs/superpowers/handoffs/2026-06-22-rollshot-vision.md`

**Interfaces:**
- Consumes: public `rollshot_vision::{VisualIndex, TemplateStore, TemplateAsset, TemplateBytes, TemplateSensitivity, TemplateSource, RealAutomationHost}`; `rollshot_automation::{validate_source, ValidationLimits, AutomationInput, Region, ProposedEditKind, execute_to_proposal, CancellationFlag, ExecutionPolicy}`; `rollshot_automation::ProposalContext`; `rollshot_edit_proposal::{ProposalId, Provenance, ProvenanceSource, ProposedEdit}`; `rollshot_automation_rquickjs::QuickJsExecutor`.
- The harness constructs the exact `TemplateMatchQuery`, prepares it before `execute_to_proposal`, and uses the same handle/region/limit in the validated JS fixture. This explicitly tests the synchronous callback boundary.

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

- [ ] **Step 2: Write the integration harness + bookmark regression case**

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

/// 60x60 deterministic textured scene. Paste a non-periodic 8x8 glyph at each
/// (x,y) in `marks` so periodic checker aliases cannot make the test pass.
fn scene_with(marks: &[(u32, u32)]) -> image::RgbaImage {
    let mut scene = image::RgbaImage::from_fn(60, 60, |x, y| {
        let v = 120 + ((x * 3 + y * 5) % 23) as u8;
        image::Rgba([v, v, v, 255])
    });
    for &(ox, oy) in marks {
        for dy in 0..8 {
            for dx in 0..8 {
                let v = ((dx * 31 + dy * 17 + dx * dy * 7) % 220) as u8;
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
    })
    .unwrap();
    store
}

fn run(
    js: &str,
    scene: image::RgbaImage,
    store: TemplateStore,
    handle_key: &str,
    handle_value: &str,
    query_limit: u32,
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
    let query = rollshot_automation::TemplateMatchQuery {
        template_handle: handle_value.to_string(),
        region: Region::Full,
        limit: query_limit,
    };
    let mut host = RealAutomationHost::new();
    host.prepare_template_match(&index, &store, &query).unwrap();
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
        40,
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

- [ ] **Step 3: Run the first end-to-end regression**

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
        80,
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
        40,
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
        run(
            BOOKMARKS_JS,
            scene,
            store_with("bookmarkStrip", tpl),
            "bookmarkStrip",
            "bookmarkStrip",
            40,
        )
    };
    let a = make();
    let b = make();
    assert_eq!(a.candidates, b.candidates);
}

#[test]
fn translated_instance_is_still_found() {
    let scene = scene_with(&[(41, 37)]);
    let tpl = template_from(&scene, 41, 37);
    let proposal = run(
        BOOKMARKS_JS,
        scene,
        store_with("bookmarkStrip", tpl),
        "bookmarkStrip",
        "bookmarkStrip",
        40,
    );
    assert_eq!(proposal.candidates.len(), 1);
    match &proposal.candidates[0].edit {
        ProposedEdit::AddRedaction { bounds } => {
            assert!((bounds.x - 41.0).abs() <= 2.0);
            assert!((bounds.y - 37.0).abs() <= 2.0);
        }
        other => panic!("expected AddRedaction, got {other:?}"),
    }
}

#[test]
fn scaled_instance_is_a_known_miss() {
    let source = scene_with(&[(6, 6)]);
    let tpl_image = image::imageops::crop_imm(&source, 6, 6, 8, 8).to_image();
    let tpl = TemplateBytes::new(8, 8, tpl_image.clone().into_raw()).unwrap();
    let scaled = image::imageops::resize(
        &tpl_image,
        16,
        16,
        image::imageops::FilterType::Triangle,
    );
    let mut scene = scene_with(&[]);
    image::imageops::replace(&mut scene, &scaled, 20, 20);
    let proposal = run(
        BOOKMARKS_JS,
        scene,
        store_with("bookmarkStrip", tpl),
        "bookmarkStrip",
        "bookmarkStrip",
        40,
    );
    assert!(proposal.candidates.is_empty());
}
```

- [ ] **Step 7: Run the full integration suite**

Run: `rtk cargo test -p rollshot-vision --test integration`
Expected: PASS (6 tests). If the blank case produces candidates, treat that as a matcher correctness failure; do not tune the fixture around a false positive. The zero-mean NCC path should return non-matches for zero-variance windows, while the JS threshold removes finite weak matches.

- [ ] **Step 8: Run the whole crate + workspace check**

Run: `rtk cargo test -p rollshot-vision`
Expected: PASS (unit + integration).
Run: `rtk cargo test --workspace`
Expected: PASS; no automation/core regressions.
Run: `rtk cargo clippy --workspace --all-targets -- -D warnings`
Expected: clean.
Run: `rtk cargo fmt --check`
Expected: clean.
Run: `rtk cargo tree -p rollshot-vision -i imageproc`
Expected: one resolved `imageproc` 0.26.x package shared with `rollshot-core`.

- [ ] **Step 9: Append the sub-project completion handoff**

Append: "PR6 done — SP1 complete. Hand-authored role-free detectors run through explicit vision preparation + `QuickJsExecutor` + cached `RealAutomationHost` and produce expected proposals on deterministic synthetic fixtures. Blank, translation, known-scale-miss, determinism, capability error, privacy, and resource-bound cases are covered. Deferred: query-plan extraction/product wiring (SP6), regionFeatures (SP2), author acquisition (SP3), inspectLayout (SP4), OCR (SP5)."

- [ ] **Step 10: Commit**

```bash
rtk git add crates/rollshot-vision/tests docs/superpowers/handoffs/2026-06-22-rollshot-vision.md
rtk git commit -m "test(vision): cover prepared QuickJS detection flow" -m "Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Test Coverage Map

```text
Task / behavior                                           Unit  Integ  E2E / smoke  Manual only
────────────────────────────────────────────────────────  ────  ─────  ───────────  ───────────
T1 / all unavailable capabilities fail explicitly         ✓     —      —            no
T2 / empty image + cached grayscale                        ✓     —      —            no
T2 / rect rounding, clamp, endpoint finite, area errors    ✓     —      —            no
T3 / TemplateBytes invariants                              ✓     —      —            no
T3 / store count/byte accounting                           ✓     —      —            no
T3 / local save-load + corrupt input                       ✓     —      —            no
T3 / export strips Sensitive + no generic Serialize        ✓     —      —            no
T4 / missing/low-info/too-large/zero-limit errors          ✓     —      —            no
T4 / equal-dimension imageproc edge case                   ✓     —      —            no
T4 / work-budget refusal before matching                   ✓     —      —            no
T4 / ZNCC ranking, bounded extraction, NMS, determinism    ✓     —      —            no
T4 / prepared callback lookup/truncation/cache miss         ✓     —      —            no
T5 / Pass/Reject/NeedsConfirm decision branches             ✓     —      —            no
T5 / self-location, repeats, area, coverage, jitter          ✓     —      —            no
T6 / validated JS -> QuickJS -> EditProposal                —     ✓      ✓            no
T6 / blank, translation, known scale miss, determinism      —     ✓      ✓            no
Product Result Workspace and real screenshot UX             —     —      —            SP6
```

### Failure modes

| New codepath | Realistic production failure | Test / handling | User-visible outcome |
|---|---|---|---|
| `VisualIndex::build` | Zero-sized or malformed upstream image | Task 2 build test; `VisionError::EmptyImage` | Explicit setup failure |
| Region conversion | NaN/∞ endpoint, empty clamp, overflow, oversized search | Task 2 rect tests; typed `CapabilityError` | Explicit detector input failure |
| Template insertion | Too many assets or excessive retained bytes | Task 3 store-limit tests; `VisionError::StoreLimit` | Explicit preset/template failure |
| Local load | Truncated/corrupt JSON or invalid RGBA invariant | Task 3 corrupt + round-trip tests; typed `VisionError` | Explicit local-data failure |
| Export | Sensitive bytes accidentally serialized | Task 3 export test + negative trait assertions | Export succeeds with bytes stripped |
| Template preparation | Missing handle, low variance, template larger than region | Task 4 negative tests; typed capability errors | Explicit detector failure |
| Matcher resource control | Score map or pixel-visit cost exceeds limits | Task 4 work-budget test before allocation | Explicit `region_too_large`; caller narrows region/template |
| `imageproc` boundary | Equal width/height would panic in library | Task 4 equal-dimension test; direct raw-dot fallback | No panic |
| NCC normalization | Zero-variance scene window or non-finite score | Unit helper/matcher tests; score dropped | No false candidate from invalid score |
| Prepared callback | Query was not prepared or asks above prepared limit | Task 4 host tests; `vision_index_unavailable` / limit error | Explicit capability failure, never silent empty |
| Self-validation | Crop does not return to source, repeats unexpectedly, jitter unstable | Task 5 branch tests; deterministic Reject/NeedsConfirm | Author flow does not auto-adopt weak template |
| QuickJS integration | Script/manifest/host query drift | Task 6 end-to-end fixture; typed execution failure | No proposal is produced |

No failure mode is both silent and uncovered. The remaining product-level error rendering is intentionally SP6.

## Performance and Resource Contract

- Preparation, not callback execution, owns detector latency. The callback path contains no image conversion, score-map allocation, disk IO, or template matching.
- Reject before work when either score positions exceed `MAX_SCORE_POSITIONS` or estimated template pixel visits exceed `MAX_TEMPLATE_MATCH_PIXEL_VISITS`.
- Score-map memory is bounded by `MAX_SCORE_POSITIONS * sizeof(f32)`. Integral moments add two `(search_width + 1) * (search_height + 1) * sizeof(f64)` buffers; document the current worst-case byte estimate beside the constants and keep it below the product worker's future host-memory budget.
- Candidate memory is bounded by `clamp(limit * PEAK_OVERSAMPLE, 64, 8192)` rather than one candidate per score-map cell.
- Template persistence is bounded by `MAX_TEMPLATE_COUNT`, `MAX_TEMPLATE_AREA`, and `MAX_TEMPLATE_STORE_BYTES`.
- Runtime diagnostics use stable `rollshot::vision::*` targets with counts/durations only. Never log handles, paths, template bytes, source bounds tied to private content, or query payloads.
- SP1 does not claim acceptable tall-image full-search latency. Full-height searches that exceed structural budgets fail explicitly; SP6 must choose bounded regions or schedule a follow-up coarse-to-fine matcher.

## Auto Decisions

### Auto decision D1 — Where does detector work run?

Context: the existing automation runtime requires sub-millisecond synchronous host callbacks, while the draft ran NCC inside the callback.
ELI10: QuickJS can stop JavaScript, but it cannot stop Rust while Rust is scanning millions of pixels. If matching happens in the callback, cancel and timeout controls are misleading.
Stakes if we pick wrong: the app can freeze beyond its timeout while processing private screenshots.
Recommendation: **1A** because explicit preparation preserves the current capability boundary and makes failure/cancellation ownership honest.
Note: options differ in kind, not coverage — no completeness score.
Pros / cons:
A) **1A — Prepare before QuickJS (recommended)** (human: ~1 day / AI: ~45 min; low risk; low maintenance) ✅ callback is bounded and testable ❌ caller must supply concrete queries.
B) **1B — Match inside callback** (human: ~2 hours / AI: ~10 min; high risk; low code maintenance) ✅ simplest wiring ❌ violates the locked runtime contract.
Net: accept a small orchestration seam to avoid an uninterruptible runtime path.

### Auto decision D2 — What persistence surface ships in SP1?

Context: the spec requires local serialization and export gating, but the draft only returned cloned record vectors.
ELI10: Returning a vector proves a transformation, not that saved data can be read back or that corrupt files fail safely. Privacy bugs happen at the actual write boundary.
Stakes if we pick wrong: sensitive template bytes can leak or local presets can become unreadable without a typed failure.
Recommendation: **2A** because explicit path APIs and round trips complete the promised boundary without adding product persistence.
Completeness: A=9/10, B=5/10.
Pros / cons:
A) **2A — Explicit save/load/export records (recommended)** (human: ~1 day / AI: ~40 min; low risk; moderate maintenance) ✅ tests real bytes-on-disk policy ❌ commits to a provisional JSON record shape.
B) **2B — Return records only** (human: ~2 hours / AI: ~10 min; medium risk; low maintenance) ✅ smaller diff ❌ defers the security-sensitive boundary while claiming it exists.
Net: serialize through named records now; defer full preset/revision persistence.

### Auto decision D3 — Which correlation score is the contract?

Context: `imageproc::CrossCorrelationNormalized` is normalized dot product, not zero-mean NCC.
ELI10: Bright images can look “similar” simply because all pixel values are positive. Subtracting each window's average makes the score respond to shape and contrast instead of brightness alone.
Stakes if we pick wrong: unrelated blank/UI regions may clear the JavaScript 0.8 thresholds and create false redactions.
Recommendation: **3A** because raw library correlation plus integral moments is the smallest technically correct wrapper.
Note: options differ in kind, not coverage — no completeness score.
Pros / cons:
A) **3A — True zero-mean NCC wrapper (recommended)** (human: ~2 days / AI: ~90 min; medium risk; moderate maintenance) ✅ threshold semantics match the design ❌ adds numeric code and memory.
B) **3B — Use `CrossCorrelationNormalized` directly** (human: ~2 hours / AI: ~10 min; high product risk; low maintenance) ✅ library-only code ❌ known high-score false positives.
Net: spend the custom-code budget only on normalization the library does not provide.

### Auto decision D4 — How are task commits and handoffs structured?

Context: each task committed before appending its handoff and omitted the required trailer.
ELI10: A handoff written after the commit is not part of the phase it describes and leaves the tree dirty. The required attribution line also has to be in the actual commit message.
Stakes if we pick wrong: execution stops at every phase with uncommitted docs and non-conforming history.
Recommendation: **4A** because it makes every task atomic and independently handoff-ready.
Completeness: A=10/10, B=4/10.
Pros / cons:
A) **4A — Handoff before atomic commit (recommended)** (human: minutes / AI: minutes; negligible risk; low maintenance) ✅ clean phase boundaries ❌ each phase touches the shared handoff file.
B) **4B — Keep post-commit notes** (human: no work / AI: no work; medium workflow risk; recurring maintenance) ✅ fewer plan edits ❌ contradicts the handoff requirement.
Net: every phase commits code, tests, and its note together.

### Auto decision D5 — How strict is TDD ordering?

Context: Task 1 implemented the stub before adding the “failing” test, and Task 6 mislabeled an already-green integration regression as failing.
ELI10: A RED step must actually fail for the reason the next code fixes. Calling a green regression “failing” makes execution evidence unreliable.
Stakes if we pick wrong: agents can claim TDD without proving that tests detect missing behavior.
Recommendation: **5A** because explicit RED/GREEN language is cheap and auditable.
Completeness: A=10/10, B=6/10.
Pros / cons:
A) **5A — Real RED for new behavior; regression label for E2E (recommended)** (human: ~1 hour / AI: ~15 min; low risk; low maintenance) ✅ honest verification ❌ slightly more step text.
B) **5B — Keep approximate ordering** (human: no work / AI: no work; medium process risk; low maintenance) ✅ shorter plan ❌ Run/Expected evidence is false.
Net: tests precede implementation where behavior is new; final integration locks an already-built path.

### Auto decision D6 — How complete is template self-validation?

Context: the draft omitted area gates, source-location verification, crop/padding jitter, and used a fixture whose entropy could not pass its own threshold.
ELI10: A crop always matches somewhere in its source image; the important question is whether it returns to the intended place and stays useful after tiny changes.
Stakes if we pick wrong: weak templates are auto-adopted and later miss sensitive content.
Recommendation: **6A** because this is the safety gate for future author-time automation.
Completeness: A=9/10, B=5/10.
Pros / cons:
A) **6A — Implement all specified deterministic gates (recommended)** (human: ~2 days / AI: ~90 min; medium risk; moderate maintenance) ✅ decision values mean what the spec says ❌ more threshold tests.
B) **6B — Keep score/entropy-only v0** (human: ~4 hours / AI: ~20 min; high product risk; low maintenance) ✅ faster initial code ❌ tautological self-match can pass bad crops.
Net: keep the author pipeline deferred, but make its pure validation primitive complete.

### Auto decision D7 — How is the serialization privacy rule enforced?

Context: the draft's test only serialized export records and did not prove asset/store types lacked `Serialize`.
ELI10: Testing the safe door does not prove there is no unlocked side door. A compile-time negative trait assertion catches someone later adding generic serialization.
Stakes if we pick wrong: a future `serde_json::to_writer(&store)` can bypass Sensitive stripping.
Recommendation: **7A** because compile-time enforcement is cheap and direct.
Completeness: A=10/10, B=6/10.
Pros / cons:
A) **7A — Negative trait assertions + byte-level export test (recommended)** (human: ~2 hours / AI: ~15 min; low risk; low maintenance) ✅ blocks the escape hatch ❌ adds one dev dependency.
B) **7B — Behavioral export test only** (human: ~1 hour / AI: ~10 min; medium risk; low maintenance) ✅ no assertion dependency ❌ generic serialization can regress unnoticed.
Net: enforce both “safe path works” and “unsafe generic path does not exist.”

### Auto decision D8 — Which matcher edge cases are mandatory?

Context: zero limit returned one item, equal-sized templates could panic in imageproc, endpoint overflow was unchecked, and the periodic fixture could alias.
ELI10: These are not exotic cases; they are direct consequences of public inputs and the selected library API. Deterministic non-periodic fixtures prevent a broken matcher from accidentally passing.
Stakes if we pick wrong: panics, incorrect result counts, or false confidence enter the runtime host.
Recommendation: **8A** because every case is small and automatable.
Completeness: A=10/10, B=6/10.
Pros / cons:
A) **8A — Cover all boundary cases (recommended)** (human: ~1 day / AI: ~35 min; low risk; low maintenance) ✅ closes known correctness holes ❌ more fixture code.
B) **8B — Happy path + missing handle only** (human: ~3 hours / AI: ~15 min; high risk; low maintenance) ✅ compact tests ❌ known panic/input bugs remain.
Net: boundary tests are part of the implementation, not optional hardening.

### Auto decision D9 — Does PR6 lock known behavior or leave it manual?

Context: the spec requires translation and a known scale miss, but the draft explicitly left scale manual.
ELI10: A known limitation is still behavior. A test prevents someone from accidentally changing thresholds or claiming scale support without review.
Stakes if we pick wrong: the demo matrix drifts and future product code may assume unsupported scale invariance.
Recommendation: **9A** because deterministic synthetic variants are cheap.
Completeness: A=10/10, B=7/10.
Pros / cons:
A) **9A — Add translation + known-scale-miss tests (recommended)** (human: ~3 hours / AI: ~20 min; low risk; low maintenance) ✅ locks both capability and limitation ❌ scale test must use a robust fixture.
B) **9B — Document scale manually** (human: minutes / AI: minutes; medium drift risk; recurring manual burden) ✅ smaller suite ❌ no executable contract.
Net: limitations deserve regression tests when the result is deterministic.

### Auto decision D10 — How is matcher work bounded?

Context: an 8-million-pixel area cap still permits hundreds of millions or billions of template pixel visits.
ELI10: Search area alone ignores template size. A 64×64 template costs 64 times more than an 8×8 template over the same screenshot.
Stakes if we pick wrong: ordinary requests can monopolize CPU despite passing “area” validation.
Recommendation: **10A** because position and pixel-visit budgets model the real loop.
Completeness: A=10/10, B=5/10.
Pros / cons:
A) **10A — Checked positions + pixel visits (recommended)** (human: ~4 hours / AI: ~20 min; low risk; low maintenance) ✅ rejects before allocation/work ❌ callers may need narrower regions.
B) **10B — Search-area cap only** (human: no work / AI: no work; high performance risk; low maintenance) ✅ one simple constant ❌ does not bound runtime.
Net: bound the operation count the algorithm actually performs.

### Auto decision D11 — How is candidate/store memory bounded?

Context: the draft collected one candidate per score cell and allowed an unbounded number of stored templates.
ELI10: Millions of scores can become hundreds of megabytes of candidate structs, and many 1 MB templates can exhaust memory even if each template is valid.
Stakes if we pick wrong: memory spikes or OOM during private-image processing.
Recommendation: **11A** because bounded top-K extraction and store ceilings preserve the same user-visible API.
Completeness: A=9/10, B=4/10.
Pros / cons:
A) **11A — Bounded candidate pool + store ceilings (recommended)** (human: ~1 day / AI: ~45 min; medium risk; moderate maintenance) ✅ predictable memory ❌ top-K oversampling becomes a documented tuning constant.
B) **11B — Collect all / unbounded store** (human: no work / AI: no work; high risk; low maintenance) ✅ simplest implementation ❌ memory scales with attacker/user input.
Net: memory ceilings are part of the host security boundary.

### Auto decision D12 — What performance evidence is sufficient for SP1?

Context: the draft acknowledged tall-image risk but had no structural performance test or callback separation.
ELI10: Wall-clock assertions are flaky in CI, but operation-count refusal and “no detector work in callback” are deterministic properties. They prove the architecture before product benchmarks exist.
Stakes if we pick wrong: tests pass while runtime cost remains unbounded or timeout ownership stays false.
Recommendation: **12A** because structural budgets are stable now; real screenshot benchmarks belong with SP6/product wiring.
Completeness: A=8/10, B=3/10.
Pros / cons:
A) **12A — Structural cost tests now, product latency benchmark later (recommended)** (human: ~4 hours / AI: ~20 min; low risk; low maintenance) ✅ stable CI evidence ❌ no claim for tall-image latency.
B) **12B — No perf checks until SP6** (human: no work / AI: no work; high carry-forward risk; low maintenance) ✅ fastest SP1 ❌ algorithmic blowups can land unnoticed.
Net: prove boundedness now and explicitly defer representative wall-clock benchmarking.

---

## Self-Review

**Spec coverage:**
- §3.1 crate/deps/dependency-direction → Task 1. §3.2 module layout → Tasks 1–6 create the listed files (`ocr.rs`/`layout.rs`/`region_features.rs` correctly absent — deferred). ✅
- §4.1 `RealAutomationHost` + capability_unavailable → Task 1 (stub) + Task 4 (prepared cached implementation). The public `AutomationHost` contract remains unchanged; the preparation seam is required by the already-implemented parent runtime's synchronous callback limit. ✅
- §4.2 `VisualIndex` (eager grayscale, reject empty, no build-options) → Task 2. ✅
- §4.3 TemplateStore/Sensitivity/TemplateBytes/serialize gate/records → Task 3, including real save/load/export boundaries and store ceilings. ✅
- §4.4 templateMatch (zero-mean NCC, to_pixel_rect rules, low-info, non-finite, NMS, anchor=center, host-no-threshold) → Task 4 (+ Task 2 for `to_pixel_rect`). ✅
- §4.5 self_validate(candidate_bounds) + source overlap + area/coverage/jitter signals + decision + ExpectedCount → Task 5. ✅
- §5 error model (VisionError + CapabilityError codes) → Task 1 (VisionError), Tasks 2/4 (codes). ✅
- §6 privacy (export strips Sensitive) → Task 3. ✅
- §7.1 unit tests → Tasks 2–5; §7.2 integration matrix → Task 4 typed errors + Task 6 bookmark/folder/blank/translation/known-scale-miss/determinism. ✅
- §8 PR breakdown (Model C, per-PR handoff notes) → Tasks 1–6 write the handoff before the atomic phase commit. ✅
- Guardrails → enforced across Tasks 2–5; restated in Global Constraints. ✅

**Placeholder scan:** No TBD/TODO. The external API check records the exact resolved `imageproc` patch release and verifies its panic/score semantics before implementation. ✅

**Type consistency:** `match_template_image` is shared by Task 4 preparation and Task 5 self-validation. `RealAutomationHost::new()` remains stable from PR1 onward; PR4 adds `prepare_template_match`. `TemplateStore::insert` consistently returns `Result`; save/load/export consistently use `&Path`; Task 6 prepares the exact query before execution. ✅

## Review Completion Summary

```text
Plan reviewed:           docs/superpowers/plans/2026-06-22-rollshot-vision-runtime-host.md
Tasks in plan:           6
Files Create/Modify:     12 create / 2 existing modify

- Step 0: Scope Challenge   — accepted as-is; no complexity threshold triggered
- Architecture Review:       3 issues, resolved by D1–D3
- Plan Structure + Code Q:   3 issues, resolved by D4–D6
- Test Review:               coverage table produced, 3 grouped gaps resolved by D7–D9
- Performance Review:        3 issues, resolved by D10–D12
- NOT in scope:              written
- What already exists:       written
- Failure modes:             0 critical silent gaps
- Parallelization:           1 sequential lane, 0 parallel lanes
- Unresolved decisions:      0
```

Plan is locked in. Execute sequentially with `superpowers:executing-plans`; `subagent-driven-development` provides no useful parallelism because every task shares the new crate and handoff file.
