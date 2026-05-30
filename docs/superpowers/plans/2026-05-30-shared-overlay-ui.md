# Shared Overlay UI/UX Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the Tauri-webview overlay (`rollshot-app`) and the native iced overlay (`rollshot-overlay`) render the live preview and crop visuals from shared sources of truth, so they stay consistent and cannot drift.

**Architecture:** A new pure-Rust crate `rollshot-overlay-core` holds the platform-independent pieces — the grow-then-follow preview viewport generator and the crop visual design tokens. Each stack renders from it (overlay wraps the preview in an iced `Handle` + draws the crop with `iced::Color`; app PNG-encodes the preview + mirrors the tokens as CSS `:root` vars). A Rust test ties the CSS values back to the Rust token consts.

**Tech Stack:** Rust (`image 0.25`), iced 0.14 (overlay only), Tauri 2 + React/TS (app). Spec: `docs/superpowers/specs/2026-05-30-overlay-app-shared-ui-design.md`.

---

## Ground Rules

- All shell commands are prefixed `rtk` per `RTK.md`.
- `rollshot-overlay-core` MUST NOT depend on iced, Tauri, a webview, `rollshot-overlay`, or `rollshot-app`. Only `image`.
- `rollshot-core` is NOT touched (it stays stitching-only).
- Only the **live** preview path changes in the app (`stitch_preview_png`); `latest_preview_png` and `final_preview_png` keep their whole-image downscale.
- Frequent commits — one per task.

## File Structure

| File | Responsibility |
|------|----------------|
| `crates/rollshot-overlay-core/Cargo.toml` | new crate manifest (`image` only) |
| `crates/rollshot-overlay-core/src/lib.rs` | module wiring |
| `crates/rollshot-overlay-core/src/preview.rs` | `preview_viewport` + `PREVIEW_WIDTH`/`PREVIEW_MAX_HEIGHT` |
| `crates/rollshot-overlay-core/src/tokens.rs` | `Rgba` + crop visual token consts + `to_css` |
| `Cargo.toml` (root) | add workspace member |
| `crates/rollshot-overlay/Cargo.toml` | add `rollshot-overlay-core` dep |
| `crates/rollshot-overlay/src/driver.rs` | `preview_viewport_handle` becomes a thin wrapper |
| `crates/rollshot-overlay/src/overlay.rs` | use shared consts; `CropCanvas` draws mask+border+guides from tokens |
| `crates/rollshot-app/src-tauri/Cargo.toml` | add `rollshot-overlay-core` dep |
| `crates/rollshot-app/src-tauri/src/session.rs` | `stitch_preview_png` uses shared viewport; `encode_rgba_png` helper |
| `crates/rollshot-app/src-tauri/src/css_token_sync.rs` | token sync test |
| `crates/rollshot-app/src-tauri/src/lib.rs` | declare the sync-test module |
| `crates/rollshot-app/src/App.css` | crop token `:root` vars; SelectionLayer rules use `var()` |

---

## Task 1: Scaffold `rollshot-overlay-core`

**Files:**
- Create: `crates/rollshot-overlay-core/Cargo.toml`
- Create: `crates/rollshot-overlay-core/src/lib.rs`
- Modify: `Cargo.toml` (root workspace `members`)

- [ ] **Step 1: Create the manifest**

`crates/rollshot-overlay-core/Cargo.toml`:

```toml
[package]
name = "rollshot-overlay-core"
version = "0.1.0"
edition = "2021"
publish = false

[dependencies]
image = { workspace = true }

[lints]
workspace = true
```

- [ ] **Step 2: Create the lib skeleton**

`crates/rollshot-overlay-core/src/lib.rs`:

```rust
//! Platform-independent overlay UI logic shared between the Tauri webview
//! overlay (`rollshot-app`) and the native iced overlay (`rollshot-overlay`):
//! the live-preview viewport generator and the crop visual design tokens, so
//! both render from one source of truth. No iced / Tauri / webview deps.

pub mod preview;
pub mod tokens;
```

(The two modules are added in Tasks 2 and 3; this won't compile until then — do
Step 3 + Task 2 + Task 3 before building.)

- [ ] **Step 3: Register the workspace member**

In root `Cargo.toml`, add `"crates/rollshot-overlay-core"` to `[workspace] members`:

```toml
members = [
    "crates/rollshot-core",
    "crates/rollshot-capture",
    "crates/rollshot-cli",
    "crates/rollshot-app/src-tauri",
    "crates/rollshot-overlay",
    "crates/rollshot-overlay-core",
]
```

(Commit happens at the end of Task 3, once the crate compiles.)

---

## Task 2: `preview_viewport` (TDD)

**Files:**
- Create: `crates/rollshot-overlay-core/src/preview.rs`

- [ ] **Step 1: Write the failing tests**

`crates/rollshot-overlay-core/src/preview.rs`:

```rust
use image::RgbaImage;

/// Fixed preview width (matches wayscrollshot's PREVIEW_MAX_WIDTH). Keeps the
/// per-frame preview texture small enough to upload stably on the
/// iced_layershell/wgpu path.
pub const PREVIEW_WIDTH: u32 = 280;
/// Cap on the preview height: the preview grows up to this, then follows the
/// bottom of the stitch.
pub const PREVIEW_MAX_HEIGHT: u32 = 480;

#[cfg(test)]
mod tests {
    use super::preview_viewport;
    use image::{Rgba, RgbaImage};

    #[test]
    fn grows_to_content_below_cap() {
        // Stitch shorter than the cap: result height is the scaled content, not
        // padded to the cap — so the preview visibly grows with scroll.
        let image = RgbaImage::from_pixel(1920, 1080, Rgba([12, 34, 56, 255]));
        let view = preview_viewport(&image, 960, 2_000);
        // 1920->960 halves width; 1080->540 < 2000 cap, so no clamp.
        assert_eq!((view.width(), view.height()), (960, 540));
    }

    #[test]
    fn caps_and_follows_bottom_for_tall_canvas() {
        let mut image = RgbaImage::new(960, 6_000);
        for y in 0..image.height() {
            for x in 0..image.width() {
                image.put_pixel(x, y, Rgba([(y % 251) as u8, (x % 251) as u8, 99, 255]));
            }
        }
        let view = preview_viewport(&image, 960, 540);
        // Capped at 540 tall, showing the bottom: first row is source row 6000-540.
        assert_eq!((view.width(), view.height()), (960, 540));
        assert_eq!(view.get_pixel(0, 0).0, [((6_000 - 540) % 251) as u8, 0, 99, 255]);
    }
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `rtk cargo test -p rollshot-overlay-core`
Expected: FAIL — `cannot find function preview_viewport`.

- [ ] **Step 3: Implement `preview_viewport`**

Add to `crates/rollshot-overlay-core/src/preview.rs` (above the `tests` module):

```rust
/// Build a wayscrollshot-style preview that grows, then follows the bottom.
///
/// Scales `image` to `width`, then takes the bottom `min(scaled_height,
/// max_height)` rows. While the stitch is short the result is short (the
/// preview visibly grows with scroll); once it would exceed `max_height` the
/// result stays bounded and tracks the latest (bottom) content. Both consumers
/// render this identically: the iced overlay wraps it in an `image::Handle`,
/// the webview app PNG-encodes it.
pub fn preview_viewport(image: &RgbaImage, width: u32, max_height: u32) -> RgbaImage {
    let width = width.max(1);
    let max_height = max_height.max(1);
    let scale = width as f32 / image.width().max(1) as f32;
    let scaled_height = ((image.height() as f32 * scale).round() as u32).max(1);
    let scaled = if image.width() == width && image.height() == scaled_height {
        image.clone()
    } else {
        image::imageops::resize(
            image,
            width,
            scaled_height,
            image::imageops::FilterType::Triangle,
        )
    };
    let out_height = scaled.height().min(max_height);
    let src_y = scaled.height() - out_height;
    image::imageops::crop_imm(&scaled, 0, src_y, width, out_height).to_image()
}
```

- [ ] **Step 4: Run to verify it passes**

Run: `rtk cargo test -p rollshot-overlay-core`
Expected: PASS (2 tests).

---

## Task 3: `tokens` module (TDD)

**Files:**
- Create: `crates/rollshot-overlay-core/src/tokens.rs`

- [ ] **Step 1: Write the failing tests**

`crates/rollshot-overlay-core/src/tokens.rs`:

```rust
//! Crop selection visual design tokens. Canonical source of truth, mirrored in
//! `crates/rollshot-app/src/App.css` `:root` and consumed by the iced overlay's
//! `CropCanvas`. The token sync test in `rollshot-app/src-tauri` asserts the CSS
//! values still match these.

/// An sRGB color: 8-bit channels + float alpha — the form both CSS
/// (`#rrggbb` / `rgba(r,g,b,a)`) and `iced::Color::from_rgba8` can express.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rgba {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: f32,
}

impl Rgba {
    pub const fn new(r: u8, g: u8, b: u8, a: f32) -> Self {
        Self { r, g, b, a }
    }

    /// CSS spelling: `#rrggbb` when opaque, else `rgba(r, g, b, a)`. Matches the
    /// exact text used in App.css so the sync test can compare by substring.
    pub fn to_css(&self) -> String {
        if self.a >= 1.0 {
            format!("#{:02x}{:02x}{:02x}", self.r, self.g, self.b)
        } else {
            format!("rgba({}, {}, {}, {})", self.r, self.g, self.b, self.a)
        }
    }
}

/// Crop rectangle border (sky-blue).
pub const CROP_BORDER: Rgba = Rgba::new(0x38, 0xbd, 0xf8, 1.0);
pub const CROP_BORDER_WIDTH: f32 = 2.0;
/// 1px white halo just outside the border.
pub const CROP_BORDER_HALO: Rgba = Rgba::new(255, 255, 255, 0.72);
/// Dark mask over everything outside the crop once a rect exists.
pub const CROP_MASK: Rgba = Rgba::new(0, 0, 0, 0.24);
/// Dim over the whole layer before any rect is drawn.
pub const CROP_DIM: Rgba = Rgba::new(0, 0, 0, 0.22);
/// Cursor crosshair guides.
pub const CROP_GUIDE: Rgba = Rgba::new(147, 197, 253, 0.48);
pub const CROP_GUIDE_WIDTH: f32 = 1.0;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn to_css_opaque_is_hex() {
        assert_eq!(CROP_BORDER.to_css(), "#38bdf8");
    }

    #[test]
    fn to_css_translucent_is_rgba() {
        assert_eq!(CROP_MASK.to_css(), "rgba(0, 0, 0, 0.24)");
        assert_eq!(CROP_GUIDE.to_css(), "rgba(147, 197, 253, 0.48)");
    }
}
```

- [ ] **Step 2: Run to verify it fails, then passes**

Run: `rtk cargo test -p rollshot-overlay-core`
Expected: this module's 2 tests + Task 2's 2 tests all PASS (the module already
contains its implementation). If `to_css` formatting differs, fix the consts/
format to match the asserted strings.

- [ ] **Step 3: Verify the crate builds clean + no reverse deps**

Run: `rtk cargo build -p rollshot-overlay-core`
Expected: compiles.

Run: `rtk cargo clippy -p rollshot-overlay-core --all-targets -- -D warnings`
Expected: clean.

Run: `rtk cargo fmt --check`
Expected: clean.

- [ ] **Step 4: Commit**

```bash
rtk git add crates/rollshot-overlay-core Cargo.toml
rtk git commit -m "feat(overlay-core): shared preview viewport + crop visual tokens"
```

---

## Task 4: Wire the overlay preview to the shared crate

**Files:**
- Modify: `crates/rollshot-overlay/Cargo.toml`
- Modify: `crates/rollshot-overlay/src/driver.rs`
- Modify: `crates/rollshot-overlay/src/overlay.rs`

- [ ] **Step 1: Add the dependency**

In `crates/rollshot-overlay/Cargo.toml`, under `[dependencies]` (the
cross-platform section, NOT the linux target one), add:

```toml
rollshot-overlay-core = { path = "../rollshot-overlay-core" }
```

So the block reads:

```toml
[dependencies]
image = { version = "0.25", features = ["png"] }
rollshot-capture = { path = "../rollshot-capture" }
rollshot-core = { path = "../rollshot-core" }
rollshot-overlay-core = { path = "../rollshot-overlay-core" }
```

- [ ] **Step 2: Replace `preview_viewport_handle`'s body with a wrapper**

In `crates/rollshot-overlay/src/driver.rs`, replace the whole
`preview_viewport_handle` function (the one taking `(image, width, max_height)`
and its doc comment) with:

```rust
/// Wrap the shared grow-then-follow preview viewport
/// (`rollshot_overlay_core::preview::preview_viewport`) as an iced image handle.
#[allow(dead_code)]
fn preview_viewport_handle(image: &image::RgbaImage, width: u32, max_height: u32) -> ImageHandle {
    let view = rollshot_overlay_core::preview::preview_viewport(image, width, max_height);
    ImageHandle::from_rgba(view.width(), view.height(), view.into_raw())
}
```

- [ ] **Step 3: Remove the moved preview tests from driver.rs**

In `crates/rollshot-overlay/src/driver.rs`, in the `#[cfg(test)] mod tests`:
- Delete the two tests `preview_viewport_handle_grows_to_content_below_cap` and
  `preview_viewport_handle_caps_and_follows_bottom_for_tall_canvas` (they now
  live in `rollshot-overlay-core`).
- Change the test `use` line from
  `use super::{overlay_stitch_config, preview_viewport_handle, stitch_stream};`
  to
  `use super::{overlay_stitch_config, stitch_stream};`
- Keep `use image::{Rgba, RgbaImage};` and the `stitch_stream_crops_and_finalizes`
  test + `scrolling_frame` helper unchanged.

- [ ] **Step 4: Use the shared preview constants in overlay.rs**

In `crates/rollshot-overlay/src/overlay.rs`, delete the two local constants and
their comment block:

```rust
// Fixed preview width (matches wayscrollshot's PREVIEW_MAX_WIDTH). ...
const PREVIEW_WIDTH: u32 = 280;
const PREVIEW_MAX_HEIGHT: u32 = 480;
```

and add this `use` near the other `use crate::...` imports at the top of the file:

```rust
use rollshot_overlay_core::preview::{PREVIEW_WIDTH, PREVIEW_MAX_HEIGHT};
```

Then, so the test module imports the constants from their new home (rather than
relying on `super::` re-import visibility), change the overlay.rs test `use` line
from:

```rust
    use super::{
        preview_viewport_size, CHROME_SPACING, PREVIEW_MAX_HEIGHT, PREVIEW_WIDTH, TOOLBAR_H,
    };
```

to:

```rust
    use super::{preview_viewport_size, CHROME_SPACING, TOOLBAR_H};
    use rollshot_overlay_core::preview::{PREVIEW_MAX_HEIGHT, PREVIEW_WIDTH};
```

- [ ] **Step 5: Verify build, tests, lints**

Run: `rtk cargo test -p rollshot-overlay`
Expected: PASS, **8 tests** (5 coords + 1 driver `stitch_stream` + 2 overlay `preview_viewport_size`). The 2 preview-image tests moved to `rollshot-overlay-core`.

Run: `rtk cargo clippy -p rollshot-overlay --all-targets -- -D warnings`
Expected: clean.

Run: `rtk cargo fmt --check`
Expected: clean.

- [ ] **Step 6: Commit**

```bash
rtk git add crates/rollshot-overlay/Cargo.toml crates/rollshot-overlay/src/driver.rs crates/rollshot-overlay/src/overlay.rs
rtk git commit -m "refactor(overlay): use rollshot-overlay-core for preview viewport"
```

---

## Task 5: Overlay crop visuals from tokens (mask + sky-blue border + guides)

**Files:**
- Modify: `crates/rollshot-overlay/src/overlay.rs`

Rendering can't be unit-tested (iced canvas), so this task is build/clippy/fmt +
manual verification.

- [ ] **Step 1: Add the tokens import + a color helper**

In `crates/rollshot-overlay/src/overlay.rs`, add near the other imports:

```rust
use rollshot_overlay_core::tokens;
```

and add this free function (e.g. just above `struct CropCanvas`):

```rust
fn token_color(c: tokens::Rgba) -> Color {
    Color::from_rgba8(c.r, c.g, c.b, c.a)
}
```

- [ ] **Step 2: Replace `CropCanvas::draw`**

Replace the `fn draw(...)` body inside `impl canvas::Program<Message> for CropCanvas`
with:

```rust
    fn draw(
        &self,
        _state: &(),
        renderer: &iced::Renderer,
        _theme: &iced::Theme,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let mut frame = canvas::Frame::new(renderer, bounds.size());

        // R3: during capture (confirmed) draw nothing — chrome lives outside the
        // crop and the region is cropped before stitching. Selection-phase only.
        if !self.confirmed {
            match self.crop {
                Some(crop) => {
                    // Dark mask over everything outside the crop (four bands),
                    // matching the app's box-shadow dimming.
                    let mask = token_color(tokens::CROP_MASK);
                    let cx = crop.x.max(0.0);
                    let cy = crop.y.max(0.0);
                    let right = (crop.x + crop.width).min(bounds.width);
                    let bottom = (crop.y + crop.height).min(bounds.height);
                    frame.fill_rectangle(Point::ORIGIN, Size::new(bounds.width, cy), mask);
                    frame.fill_rectangle(
                        Point::new(0.0, bottom),
                        Size::new(bounds.width, (bounds.height - bottom).max(0.0)),
                        mask,
                    );
                    frame.fill_rectangle(
                        Point::new(0.0, cy),
                        Size::new(cx, crop.height),
                        mask,
                    );
                    frame.fill_rectangle(
                        Point::new(right, cy),
                        Size::new((bounds.width - right).max(0.0), crop.height),
                        mask,
                    );

                    // 1px white halo just outside the border.
                    let bw = tokens::CROP_BORDER_WIDTH;
                    let halo = canvas::Stroke::default()
                        .with_color(token_color(tokens::CROP_BORDER_HALO))
                        .with_width(1.0);
                    frame.stroke_rectangle(
                        Point::new(crop.x - bw, crop.y - bw),
                        Size::new(crop.width + bw * 2.0, crop.height + bw * 2.0),
                        halo,
                    );
                    // Sky-blue crop border.
                    let border = canvas::Stroke::default()
                        .with_color(token_color(tokens::CROP_BORDER))
                        .with_width(bw);
                    frame.stroke_rectangle(
                        Point::new(crop.x, crop.y),
                        Size::new(crop.width, crop.height),
                        border,
                    );
                }
                None => {
                    // Dim the whole layer before a rect is drawn.
                    frame.fill_rectangle(Point::ORIGIN, bounds.size(), token_color(tokens::CROP_DIM));
                }
            }

            // Cursor crosshair guides.
            if let Some(pos) = cursor.position_in(bounds) {
                let guide = token_color(tokens::CROP_GUIDE);
                let gw = tokens::CROP_GUIDE_WIDTH;
                frame.fill_rectangle(Point::new(0.0, pos.y), Size::new(bounds.width, gw), guide);
                frame.fill_rectangle(Point::new(pos.x, 0.0), Size::new(gw, bounds.height), guide);
            }
        }

        vec![frame.into_geometry()]
    }
```

- [ ] **Step 3: Verify build, lints**

Run: `rtk cargo build -p rollshot-overlay`
Expected: compiles. (If `Color::WHITE` is now unused that's fine — `Color` is
still used elsewhere, so no unused-import.)

Run: `rtk cargo clippy -p rollshot-overlay --all-targets -- -D warnings`
Expected: clean.

Run: `rtk cargo fmt --check`
Expected: clean.

- [ ] **Step 4: Manual check (KDE 6, optional now)**

Run: `rtk cargo run --release -p rollshot-overlay --bin capture_overlay`
Expected: during crop selection, everything outside the drag rectangle is dimmed
and the rectangle has a sky-blue border with a faint white halo + crosshair
guides — matching the app's selection look.

- [ ] **Step 5: Commit**

```bash
rtk git add crates/rollshot-overlay/src/overlay.rs
rtk git commit -m "feat(overlay): crop mask + sky-blue border + guides from shared tokens"
```

---

## Task 6: Wire the app live preview to the shared crate

**Files:**
- Modify: `crates/rollshot-app/src-tauri/Cargo.toml`
- Modify: `crates/rollshot-app/src-tauri/src/session.rs`

- [ ] **Step 1: Add the dependency**

In `crates/rollshot-app/src-tauri/Cargo.toml`, under `[dependencies]`, add:

```toml
rollshot-overlay-core = { path = "../../rollshot-overlay-core" }
```

- [ ] **Step 2: Add a raw-PNG encode helper**

In `crates/rollshot-app/src-tauri/src/session.rs`, add next to
`encode_preview_image_png`:

```rust
fn encode_rgba_png(image: &RgbaImage) -> Result<Vec<u8>, String> {
    let mut cursor = std::io::Cursor::new(Vec::new());
    image
        .write_to(&mut cursor, image::ImageFormat::Png)
        .map_err(|err| format!("failed to encode preview png: {err}"))?;
    Ok(cursor.into_inner())
}
```

- [ ] **Step 3: Switch `stitch_preview_png` to the shared viewport**

Replace the body of `stitch_preview_png` with:

```rust
    pub fn stitch_preview_png(&self, max_edge: u32) -> Result<Option<Vec<u8>>, String> {
        // The live stitch preview uses the shared fixed grow-then-follow viewport
        // (consistent with the native overlay), so the caller's max_edge no
        // longer applies here.
        let _ = max_edge;
        let image = {
            let mut inner = self
                .inner
                .lock()
                .map_err(|_| "session lock poisoned".to_string())?;
            inner
                .stitcher
                .as_mut()
                .and_then(|s| s.full_image())
                .cloned()
        };
        image
            .as_ref()
            .map(|image| {
                let view = rollshot_overlay_core::preview::preview_viewport(
                    image,
                    rollshot_overlay_core::preview::PREVIEW_WIDTH,
                    rollshot_overlay_core::preview::PREVIEW_MAX_HEIGHT,
                );
                encode_rgba_png(&view)
            })
            .transpose()
    }
```

(`get_stitch_preview` in `commands.rs` still passes `max_edge`; leaving the
signature intact avoids a frontend/Tauri-binding change. `latest_preview_png`
and `final_preview_png` are unchanged.)

- [ ] **Step 4: Verify build, tests, lints**

Run: `rtk cargo test -p rollshot-app`
Expected: PASS (existing session tests; `latest_preview_png_resizes_large_frame`
still green — it tests the unchanged latest path).

Run: `rtk cargo clippy -p rollshot-app --all-targets -- -D warnings`
Expected: clean.

Run: `rtk cargo fmt --check`
Expected: clean.

- [ ] **Step 5: Commit**

```bash
rtk git add crates/rollshot-app/src-tauri/Cargo.toml crates/rollshot-app/src-tauri/src/session.rs
rtk git commit -m "feat(app): live stitch preview uses shared grow-then-follow viewport"
```

---

## Task 7: App crop tokens in CSS + sync test

**Files:**
- Modify: `crates/rollshot-app/src/App.css`
- Create: `crates/rollshot-app/src-tauri/src/css_token_sync.rs`
- Modify: `crates/rollshot-app/src-tauri/src/lib.rs`

- [ ] **Step 1: Add the crop token vars to `:root`**

In `crates/rollshot-app/src/App.css`, inside the existing `:root { ... }` block,
immediately after the `--radius: 0.5rem;` line, insert:

```css
  /* Crop overlay tokens — kept in sync with rollshot_overlay_core::tokens by the
     sync test in crates/rollshot-app/src-tauri/src/css_token_sync.rs. */
  --crop-border: #38bdf8;
  --crop-border-width: 2px;
  --crop-border-halo: rgba(255, 255, 255, 0.72);
  --crop-mask: rgba(0, 0, 0, 0.24);
  --crop-dim: rgba(0, 0, 0, 0.22);
  --crop-guide: rgba(147, 197, 253, 0.48);
  --crop-guide-width: 1px;
```

- [ ] **Step 2: Point the SelectionLayer rules at the vars**

In the same file, change these rules to consume the vars:

`.selection-dim` background:
```css
.selection-dim {
  position: absolute;
  inset: 0;
  pointer-events: none;
  background: var(--crop-dim);
}
```

`.selection-guide` background:
```css
.selection-guide {
  position: absolute;
  pointer-events: none;
  background: var(--crop-guide);
}
```

`.selection-guide-x` height and `.selection-guide-y` width:
```css
.selection-guide-x {
  left: 0;
  right: 0;
  height: var(--crop-guide-width);
}

.selection-guide-y {
  top: 0;
  bottom: 0;
  width: var(--crop-guide-width);
}
```

`.selection-box` border + box-shadow:
```css
.selection-box {
  position: absolute;
  pointer-events: none;
  border: var(--crop-border-width) solid var(--crop-border);
  background: transparent;
  box-shadow:
    0 0 0 1px var(--crop-border-halo),
    0 0 0 9999px var(--crop-mask);
}
```

- [ ] **Step 3: Write the failing sync test**

Create `crates/rollshot-app/src-tauri/src/css_token_sync.rs`:

```rust
//! Asserts the crop visual tokens in `App.css` match the canonical Rust consts
//! in `rollshot_overlay_core::tokens`. Drift on either side fails this test.

#[cfg(test)]
mod tests {
    use rollshot_overlay_core::tokens;

    const CSS: &str = include_str!("../../src/App.css");

    fn assert_var(name: &str, value: &str) {
        let needle = format!("{name}: {value};");
        assert!(
            CSS.contains(&needle),
            "App.css :root is missing or drifted from `{needle}` \
             (rollshot_overlay_core::tokens is the source of truth)"
        );
    }

    #[test]
    fn css_crop_tokens_match_rust_tokens() {
        assert_var("--crop-border", &tokens::CROP_BORDER.to_css());
        assert_var("--crop-border-width", &format!("{}px", tokens::CROP_BORDER_WIDTH));
        assert_var("--crop-border-halo", &tokens::CROP_BORDER_HALO.to_css());
        assert_var("--crop-mask", &tokens::CROP_MASK.to_css());
        assert_var("--crop-dim", &tokens::CROP_DIM.to_css());
        assert_var("--crop-guide", &tokens::CROP_GUIDE.to_css());
        assert_var("--crop-guide-width", &format!("{}px", tokens::CROP_GUIDE_WIDTH));
    }
}
```

In `crates/rollshot-app/src-tauri/src/lib.rs`, add this line with the other
top-level `mod` declarations:

```rust
#[cfg(test)]
mod css_token_sync;
```

- [ ] **Step 4: Run the sync test**

Run: `rtk cargo test -p rollshot-app css_crop_tokens_match_rust_tokens`
Expected: PASS. If it fails, the failure message names the exact `--crop-*: …;`
string the CSS must contain — make App.css match (this is the drift guard
working).

- [ ] **Step 5: Frontend checks**

Run: `rtk pnpm --dir crates/rollshot-app run typecheck`
Expected: clean (CSS-only change; no TS impact).

Run: `rtk pnpm --dir crates/rollshot-app test`
Expected: PASS (SelectionLayer tests assert structure/classNames, not literal colors).

- [ ] **Step 6: Commit**

```bash
rtk git add crates/rollshot-app/src/App.css crates/rollshot-app/src-tauri/src/css_token_sync.rs crates/rollshot-app/src-tauri/src/lib.rs
rtk git commit -m "feat(app): crop visual tokens via CSS vars + Rust sync test"
```

---

## Task 8: Full verification

**Files:** none (verification only)

- [ ] **Step 1: Workspace-wide Rust verification**

Run: `rtk cargo test`
Expected: PASS (workspace).

Run: `rtk cargo fmt --check`
Expected: clean.

Run: `rtk cargo clippy --workspace --all-targets -- -D warnings`
Expected: clean.

- [ ] **Step 2: Frontend verification**

Run: `rtk pnpm --dir crates/rollshot-app run typecheck`
Run: `rtk pnpm --dir crates/rollshot-app test`
Run: `rtk pnpm --dir crates/rollshot-app run build`
Expected: all clean.

- [ ] **Step 3: Manual parity check (optional, needs a desktop session)**

- App (`rtk pnpm --dir crates/rollshot-app run tauri dev` or the built app):
  during stitching the live preview is a fixed-width strip that grows then
  follows the bottom — matching the overlay.
- Overlay (`rtk cargo run --release -p rollshot-overlay --bin capture_overlay`,
  KDE 6): crop selection shows the dark mask + sky-blue border + guides —
  matching the app.

- [ ] **Step 4: Mark the spec done**

In `docs/superpowers/specs/2026-05-30-overlay-app-shared-ui-design.md`, append a
short note under "Scope" that pillars 1 + 2 landed (preview sharing + crop
tokens) and the crate exists; pillar 3 (geometry fixtures) remains follow-up.

```bash
rtk git add docs/superpowers/specs/2026-05-30-overlay-app-shared-ui-design.md
rtk git commit -m "docs(spec): mark shared-overlay pillars 1+2 implemented"
```

---

## Success Criteria

- `rollshot-overlay-core` builds (cross-platform); `preview_viewport` + `tokens`
  unit-tested; no dep on iced/Tauri/overlay/app.
- Overlay live preview and app live preview both come from
  `preview_viewport` (identical pixels).
- Overlay crop selection shows mask + sky-blue border (+ guides), driven by
  `rollshot_overlay_core::tokens`; App.css consumes the same values via `:root`
  vars; the sync test enforces they match.
- `rollshot-core` untouched; only `stitch_preview_png` changed in the app.
- Workspace `cargo test` / `clippy -D warnings` / `fmt --check` clean; frontend
  typecheck / test / build clean.
