# Dynamic Preview Height Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the fixed preview height cap with `min(crop_height, available_band_space)` on both Linux overlay and macOS webview, and unify preview width to 280px.

**Architecture:** The `viewport_preview()` function in `rollshot-overlay-core` is unchanged — it already aspect-fits content into whatever viewport dimensions are requested. Only the callers that compute the requested dimensions change: `preview_viewport_size()` in the Linux overlay, and `CaptureOverlay.tsx` in the macOS webview.

**Tech Stack:** Rust (rollshot-overlay, rollshot-overlay-core), TypeScript/React (rollshot-app)

**Spec:** `docs/superpowers/specs/2026-06-02-dynamic-preview-height-design.md`

---

## File Map

| File | Action | Responsibility |
|------|--------|----------------|
| `crates/rollshot-overlay/src/overlay.rs` | Modify | Remove `PREVIEW_MAX_HEIGHT` cap from `preview_viewport_size()`, update tests |
| `crates/rollshot-app/src/components/CaptureOverlay.tsx` | Modify | Replace fixed `MAX_PREVIEW_SIZE` with dynamic `{ width: 280, height: region.height }` |
| `crates/rollshot-app/src/overlay/placement.test.ts` | Modify | Update test fixture preview size from `{180, 260}` to `{280, ...}` |

**Unchanged (verified):**
- `crates/rollshot-overlay-core/src/preview.rs` — `PREVIEW_WIDTH`, `PREVIEW_MAX_HEIGHT` constants stay; `viewport_preview()` logic untouched
- `crates/rollshot-overlay/src/driver.rs` — passes through whatever size `preview_viewport_size()` returns
- `crates/rollshot-app/src-tauri/src/session.rs` — `stitch_preview_png()` passes through caller's dimensions; existing test uses `PREVIEW_MAX_HEIGHT` directly (separate code path, unchanged)
- `crates/rollshot-app/src/overlay/placement.ts` — `fitPreviewSizeToRegion()` and `choosePreviewPlacement()` logic unchanged
- `crates/rollshot-app/src/components/NativeCaptureFlow.tsx` — no preview size references
- `crates/rollshot-app/src/api/capture.test.ts` — tests the API wrapper with arbitrary values, not sizing logic

---

### Task 1: Linux overlay — remove `PREVIEW_MAX_HEIGHT` cap

**Files:**
- Modify: `crates/rollshot-overlay/src/overlay.rs:20,493-516,700-739`

- [ ] **Step 1: Update the failing test to expect new behavior**

In `crates/rollshot-overlay/src/overlay.rs`, add a new test that exercises the crop-height cap (tall narrow crop with ample band space):

```rust
    #[test]
    fn preview_viewport_caps_height_at_crop_height() {
        // Tall narrow crop (200x600) with lots of space on the right band.
        // Band::Right wins (right=2260, area=3,254,400).
        // available_height = 1440-100 = 1340, band_height = 1340-50-8 = 1282.
        // New: max_height = min(1282, 600) = 600.
        // aspect = 200/600 = 0.333, max_aspect = 280/600 = 0.467.
        // aspect < max_aspect → width = round(600*0.333) = 200, height = 600.
        let crop = Rectangle {
            x: 100.0,
            y: 100.0,
            width: 200.0,
            height: 600.0,
        };
        let window = Size::new(2560.0, 1440.0);

        let viewport = preview_viewport_size(crop, window);

        assert_eq!(viewport.width, 200);
        assert_eq!(viewport.height, 600);
    }
```

Add this test after the existing `preview_viewport_clamps_width_to_side_band_and_preserves_aspect` test (after line 739).

- [ ] **Step 2: Run the new test to verify it fails**

Run: `rtk cargo test -p rollshot-overlay preview_viewport_caps_height_at_crop_height`
Expected: FAIL — old code clamps to `PREVIEW_MAX_HEIGHT=480`, producing `160x480` instead of `200x600`.

- [ ] **Step 3: Remove `PREVIEW_MAX_HEIGHT` import and apply crop-height cap**

In `crates/rollshot-overlay/src/overlay.rs`, change the import on line 20:

```rust
// Before:
use rollshot_overlay_core::preview::{PREVIEW_MAX_HEIGHT, PREVIEW_WIDTH};

// After:
use rollshot_overlay_core::preview::PREVIEW_WIDTH;
```

In `preview_viewport_size()` (line 513), replace the `PREVIEW_MAX_HEIGHT` clamp:

```rust
// Before:
    let max_height = band_height.clamp(1, PREVIEW_MAX_HEIGHT) as f32;

// After:
    let crop_h = crop.height.max(1.0);
    let max_height = (band_height as f32).min(crop_h);
```

- [ ] **Step 4: Run all overlay tests to verify**

Run: `rtk cargo test -p rollshot-overlay`
Expected: All tests pass. The existing tests (`preview_viewport_uses_fixed_width_and_bottom_band_height` and `preview_viewport_clamps_width_to_side_band_and_preserves_aspect`) still pass because the band height is already less than the crop height in those scenarios.

- [ ] **Step 5: Run clippy**

Run: `rtk cargo clippy -p rollshot-overlay --all-targets -- -D warnings`
Expected: No warnings.

- [ ] **Step 6: Commit**

```bash
git add crates/rollshot-overlay/src/overlay.rs
git commit -m "feat(overlay): cap preview height at crop height instead of fixed 480px"
```

---

### Task 2: macOS webview — dynamic preview height and 280px width

**Files:**
- Modify: `crates/rollshot-app/src/components/CaptureOverlay.tsx:27,110-113,273-276`

- [ ] **Step 1: Replace `MAX_PREVIEW_SIZE` constant with `PREVIEW_WIDTH`**

In `crates/rollshot-app/src/components/CaptureOverlay.tsx`, replace line 27:

```ts
// Before:
const MAX_PREVIEW_SIZE = { width: 180, height: 260 }

// After:
const PREVIEW_WIDTH = 280
```

- [ ] **Step 2: Update the polling loop to use dynamic height**

In the same file, update the polling `useEffect` (lines 109-113):

```ts
// Before:
        if (nextStatus.state === 'stitching') {
          const previewSize = fitPreviewSizeToRegion({
            region: nextStatus.region,
            maxPreview: MAX_PREVIEW_SIZE,
          })

// After:
        if (nextStatus.state === 'stitching') {
          const previewSize = fitPreviewSizeToRegion({
            region: nextStatus.region,
            maxPreview: { width: PREVIEW_WIDTH, height: nextStatus.region.height },
          })
```

- [ ] **Step 3: Update the placement `useMemo` to use dynamic height**

In the same file, update the `placement` memo (lines 273-276):

```ts
// Before:
    const previewSize = fitPreviewSizeToRegion({
      region: activeRegionRect,
      maxPreview: MAX_PREVIEW_SIZE,
    })

// After:
    const previewSize = fitPreviewSizeToRegion({
      region: activeRegionRect,
      maxPreview: { width: PREVIEW_WIDTH, height: activeRegionRect.height },
    })
```

- [ ] **Step 4: Run typecheck**

Run: `rtk pnpm --dir crates/rollshot-app run typecheck`
Expected: No errors.

- [ ] **Step 5: Run frontend tests**

Run: `rtk pnpm --dir crates/rollshot-app test`
Expected: All tests pass. The `placement.test.ts` tests use their own fixture values (`{ width: 180, height: 260 }`) and are not affected by the constant rename. The `capture.test.ts` tests the API wrapper with arbitrary values and is unaffected.

- [ ] **Step 6: Commit**

```bash
git add crates/rollshot-app/src/components/CaptureOverlay.tsx
git commit -m "feat(app): use 280px width and dynamic crop-height for preview on macOS"
```

---

### Task 3: Update placement tests for new preview width

**Files:**
- Modify: `crates/rollshot-app/src/overlay/placement.test.ts:5,88-105`

- [ ] **Step 1: Update the `fitPreviewSizeToRegion` test fixtures to use 280px width**

In `crates/rollshot-app/src/overlay/placement.test.ts`, update the two `fitPreviewSizeToRegion` tests (lines 88-105) to use the new 280px width and dynamic height:

```ts
// Before:
describe('fitPreviewSizeToRegion', () => {
  it('keeps a wide crop from filling a tall preview box', () => {
    expect(
      fitPreviewSizeToRegion({
        region: { width: 2400, height: 900 },
        maxPreview: { width: 180, height: 260 },
      }),
    ).toEqual({ width: 180, height: 68 })
  })

  it('reduces width for a tall crop instead of letterboxing horizontally', () => {
    expect(
      fitPreviewSizeToRegion({
        region: { width: 400, height: 1200 },
        maxPreview: { width: 180, height: 260 },
      }),
    ).toEqual({ width: 87, height: 260 })
  })
})

// After:
describe('fitPreviewSizeToRegion', () => {
  it('keeps a wide crop from filling a tall preview box', () => {
    expect(
      fitPreviewSizeToRegion({
        region: { width: 2400, height: 900 },
        maxPreview: { width: 280, height: 900 },
      }),
    ).toEqual({ width: 280, height: 105 })
  })

  it('reduces width for a tall crop instead of letterboxing horizontally', () => {
    expect(
      fitPreviewSizeToRegion({
        region: { width: 400, height: 1200 },
        maxPreview: { width: 280, height: 1200 },
      }),
    ).toEqual({ width: 280, height: 840 })
  })
})
```

Math verification:
- Wide crop (2400x900): aspect=2.667, max_aspect=280/900=0.311 → aspect≥max_aspect → w=280, h=round(280/2.667)=105
- Tall crop (400x1200): aspect=0.333, max_aspect=280/1200=0.233 → aspect≥max_aspect → w=280, h=round(280/0.333)=840

- [ ] **Step 2: Run the placement tests to verify**

Run: `rtk pnpm --dir crates/rollshot-app test -- placement.test`
Expected: All tests pass.

- [ ] **Step 3: Commit**

```bash
git add crates/rollshot-app/src/overlay/placement.test.ts
git commit -m "test(app): update placement test fixtures for 280px preview width"
```

---

### Task 4: Final verification

- [ ] **Step 1: Run full Rust test suite**

Run: `rtk cargo test --workspace`
Expected: All tests pass.

- [ ] **Step 2: Run Rust formatting check**

Run: `rtk cargo fmt --check`
Expected: No formatting issues.

- [ ] **Step 3: Run Rust clippy**

Run: `rtk cargo clippy --workspace --all-targets -- -D warnings`
Expected: No warnings.

- [ ] **Step 4: Run frontend typecheck**

Run: `rtk pnpm --dir crates/rollshot-app run typecheck`
Expected: No errors.

- [ ] **Step 5: Run frontend tests**

Run: `rtk pnpm --dir crates/rollshot-app test`
Expected: All tests pass.

- [ ] **Step 6: Run frontend build**

Run: `rtk pnpm --dir crates/rollshot-app run build`
Expected: Build succeeds.
