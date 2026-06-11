# Long-Shot Callouts and Image Document Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build the headless `rollshot-image-document` crate (annotations, history, hit-testing, flattening) and extend the iced Result Workspace in `rollshot-app` into a non-destructive annotation editor with Number Callouts, Text Notes, Opaque Redactions, a Navigator drawer, undo/redo, and annotated Copy/Save As.

**Spec:** `docs/superpowers/specs/2026-06-11-long-shot-callouts-image-document-design.md` (live for this plan; includes product-review amendments D1 compact renumbering and D2 labeled output cluster).

**Architecture:** A new framework-neutral crate owns the immutable source image, annotation graph, snapshot-based undo history, geometry/hit-testing, Navigator ordering, and full-resolution flattening. A shared `RenderShape` model is the single source of geometry truth: the crate rasterizes shapes for flattened output (cosmic-text + a small AA rasterizer), and the iced canvas maps the same shapes to live vector drawing — so live overlay and export cannot diverge. `rollshot-app` keeps all UI/session state (active tool, drafts, selection, viewport) and submits one completed document edit per gesture.

**Tech Stack:** Rust, iced 0.14 (`canvas`, `image`, `tokio` features), `image` 0.25, `cosmic-text` 0.15 (text shaping/raster with system-font CJK fallback), vendored DejaVu Sans fonts.

---

## Verified API facts (do not re-derive)

- iced 0.14 `canvas::Program`: `update(&self, &mut State, &Event, Rectangle, Cursor) -> Option<Action<Message>>` with `Action::publish(msg)`, `.and_capture()`; `draw(&self, &State, &Renderer, &Theme, Rectangle, Cursor) -> Vec<Geometry>`. `canvas::Event` is the standard `iced::Event`. Reference in-repo: `crates/rollshot-iced-overlay/src/app.rs:175` (`CropCanvas`).
- `iced::widget::operation::focus(id) -> Task<T>` and `operation::scroll_to(id, AbsoluteOffset)` exist (`scroll_to` already used in `result_workspace/mod.rs`).
- `iced::widget::text_editor` is multi-line, has `.id(impl Into<widget::Id>)`, `.on_action(...)`, `.key_binding(...)` with `Binding::Custom(Message)` / `Binding::from_key_press(event)`, and implements `operation::Focusable`. `Content::with_text(&str)`, `content.text() -> String`.
- `iced::application(...).font(bytes)` and `iced::daemon(...).font(bytes)` load custom fonts. `iced::Font::with_name(name)` is `const`.
- cosmic-text 0.15: `Buffer::set_text(&mut self, &mut FontSystem, text, attrs: &Attrs, shaping: Shaping, alignment: Option<Align>)`; `Buffer::draw(&self, &mut FontSystem, &mut SwashCache, Color, FnMut(i32, i32, u32, u32, Color))`; `Color(pub u32)`; font data loads via `font_system.db_mut().load_font_data(Vec<u8>)` (fontdb).
- DejaVu fonts available at `/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf` and `DejaVuSans-Bold.ttf`.
- The macOS product daemon (`crates/rollshot-app/src/macos_product.rs`) reuses `result_workspace::{update, view, subscription}`, `ResultWorkspace`, `ResultDocument`, `build_display_handle`, and `viewport::*` — these paths must stay importable after the refactor.
- All shell commands in this repo are prefixed with `rtk` (see AGENTS.md §6).

## Design decisions locked by review

- **D1 (spec §6/§9.2):** deleting a Number Callout compactly renumbers remaining callouts preserving relative order; next allocation = highest remaining + 1; deletion + renumbering = one undo entry; undo restores exact prior numbering.
- **D2 (spec §8.1):** creation tools, Undo/Redo, Navigator are icon buttons; Copy ▾ / Save As / Reveal keep text labels.
- **Dirty semantics:** `ImageDocument` exposes a `state_id` that changes on every commit/undo/redo and is restored by undo/redo. Dirty = `state_id != saved_state_id` recorded at the last successful Save As (or construction). Undo back to the saved state is clean.
- **Number creation direction:** press anchors the **tip** at the pressed point (the thing being annotated); dragging moves the **bubble** away. A plain click leaves them coincident (stamp, no leader).
- **Long-image rule:** Navigator defaults open when `height > 2 × width` — the same rule `viewport::default_zoom` already uses for FitWidth (extracted as `viewport::is_tall_image`).
- **No canvas geometry cache:** culling against the visible viewport (spec §11.1) conflicts with iced `canvas::Cache` (scroll changes the culled set but not the cache key). Committed annotations are re-tessellated per frame from culled shapes — trivially cheap at the ≤100-annotation scale.

## File structure

**Create — `crates/rollshot-image-document/`:**

```
Cargo.toml
README.md
assets/fonts/DejaVuSans.ttf          (vendored)
assets/fonts/DejaVuSans-Bold.ttf     (vendored)
assets/fonts/LICENSE-DejaVu          (vendored license)
src/lib.rs          module decls + re-exports
src/geometry.rs     ImagePoint, ImageRect, Rgba8
src/annotation.rs   AnnotationId, Annotation
src/style.rs        visual default constants + font bytes (REVIEWED DELIVERABLE)
src/text.rs         cosmic-text measurement + block rasterization
src/document.rs     ImageDocument, edits, compact renumbering, history
src/hit.rs          Hit, HitPart, ResizeHandle, hit_test, redaction_handles
src/navigator.rs    NavigatorItem, navigator_items
src/shapes.rs       RenderShape, annotation_shapes, annotation_bounds, text_plate_rect
src/raster.rs       AA fill primitives (rect/circle/ring/triangle)
src/flatten.rs      flatten_onto
```

**Modify — `crates/rollshot-app/src/result_workspace/`:**

```
mod.rs          ResultWorkspace struct, run(), re-exports (kept stable for macos_product.rs)
document.rs     NEW — ResultDocument, close decision, dirty rules, save-name
update.rs       NEW — Message enum, update(), gesture handlers, subscription, key mapping
view.rs         NEW — workspace chrome, toolbar, copy menu, modal, status bar
canvas.rs       NEW — EditorState, DragState, TextDraft, AnnotationCanvas, drag preview math
navigator.rs    NEW — drawer view + jump offset math
actions.rs      existing (unchanged API; callers pass flattened images)
viewport.rs     existing + is_tall_image
```

Also modify: root `Cargo.toml` (workspace member), `crates/rollshot-app/Cargo.toml` (dependency + iced features), `crates/rollshot-app/src/macos_product.rs` (font loading only).

---

# Phase 1 — `rollshot-image-document` crate

### Task 1: Crate scaffold, vendored fonts, README

**Files:**
- Modify: `Cargo.toml` (workspace root)
- Create: `crates/rollshot-image-document/Cargo.toml`
- Create: `crates/rollshot-image-document/src/lib.rs`
- Create: `crates/rollshot-image-document/README.md`
- Create: `crates/rollshot-image-document/assets/fonts/` (3 files)

- [ ] **Step 1: Add workspace member**

In root `Cargo.toml`, add to `[workspace] members` after `"crates/rollshot-core"`:

```toml
    "crates/rollshot-image-document",
```

- [ ] **Step 2: Vendor fonts and license**

```bash
mkdir -p crates/rollshot-image-document/assets/fonts
cp /usr/share/fonts/truetype/dejavu/DejaVuSans.ttf crates/rollshot-image-document/assets/fonts/
cp /usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf crates/rollshot-image-document/assets/fonts/
cp /usr/share/doc/fonts-dejavu-core/copyright crates/rollshot-image-document/assets/fonts/LICENSE-DejaVu
```

If the copyright file is missing, fetch the license text from https://dejavu-fonts.github.io/License.html and save it as `LICENSE-DejaVu`. The DejaVu license (Bitstream Vera derivative) permits redistribution and embedding with the notice retained.

- [ ] **Step 3: Write `crates/rollshot-image-document/Cargo.toml`**

```toml
[package]
name = "rollshot-image-document"
version.workspace = true
edition.workspace = true
license.workspace = true
repository.workspace = true
rust-version.workspace = true

[dependencies]
image = { workspace = true }
cosmic-text = "0.15"
thiserror = { workspace = true }

[lints]
workspace = true
```

- [ ] **Step 4: Write `src/lib.rs`**

```rust
//! Headless, framework-neutral, non-destructive image document and editing
//! engine. Owns the immutable source image, the annotation graph, history,
//! geometry, and flattened rendering. Contains no UI, windowing, clipboard,
//! or capture code — see README.md for the responsibility boundary.

mod annotation;
mod document;
mod flatten;
mod geometry;
mod hit;
mod navigator;
mod raster;
mod shapes;
pub mod style;
mod text;

pub use annotation::{Annotation, AnnotationId};
pub use document::{EditError, ImageDocument, HISTORY_LIMIT};
pub use geometry::{ImagePoint, ImageRect, Rgba8};
pub use hit::{redaction_handles, Hit, HitPart, ResizeHandle};
pub use navigator::NavigatorItem;
pub use shapes::{
    annotation_bounds, annotation_shapes, text_plate_rect, RenderShape, TextAnchor,
};
pub use text::measure_block;
```

(Modules don't exist yet — create each as an empty file so the crate compiles: `geometry.rs`, `annotation.rs`, `style.rs`, `text.rs`, `document.rs`, `hit.rs`, `navigator.rs`, `shapes.rs`, `raster.rs`, `flatten.rs` — and comment out the `pub use` lines until their task lands. Each subsequent task uncomments its own re-exports.)

- [ ] **Step 5: Write `README.md`**

```markdown
# rollshot-image-document

A headless, framework-neutral, non-destructive image document and editing
engine. The first consumer is Rollshot's Result Workspace (Long-Shot
Callouts), but the document is valid for any raster image.

## Owns

- The immutable source image (never modified by document edits).
- The annotation graph (Number Callouts, Text Notes, Opaque Redactions)
  with stable annotation IDs.
- Number Callout sequence state, including compact renumbering on delete.
- Image-space geometry, hit-testing, and Navigator ordering.
- Undo/redo history (snapshot-based, max 100 entries).
- Flattening the document into an annotated full-resolution image.
- The shared `RenderShape` geometry model used by both the flattened output
  and any live overlay renderer, so the two cannot diverge.

## Must NOT depend on or contain

- iced or any UI framework.
- Active tools, hover state, pointer state, or drag gestures.
- Zoom, scroll offset, viewport layout, or editor focus.
- Clipboard, file dialogs, file revealing, or platform APIs.
- Capture, stitching, or OCR execution.

The crate receives **completed** edits from an editor. A drag gesture in a UI
produces exactly one document edit on release; pointer movement never enters
this crate or its history.

## Fonts

`assets/fonts/` vendors DejaVu Sans (regular + bold) as the deterministic
baseline for flattened text; cosmic-text falls back to system fonts for
glyphs DejaVu lacks (e.g. CJK). See `assets/fonts/LICENSE-DejaVu`.
```

- [ ] **Step 6: Verify it builds**

Run: `rtk cargo check -p rollshot-image-document`
Expected: clean check (empty modules, commented re-exports).

- [ ] **Step 7: Commit**

```bash
rtk git add Cargo.toml crates/rollshot-image-document
rtk git commit -m "feat(image-document): scaffold headless image document crate"
```

---

### Task 2: Geometry types

**Files:**
- Modify: `crates/rollshot-image-document/src/geometry.rs`
- Modify: `crates/rollshot-image-document/src/lib.rs` (uncomment `pub use geometry::...`)

- [ ] **Step 1: Write failing tests** (bottom of `geometry.rs`)

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_corners_normalizes_inverted_drag() {
        let r = ImageRect::from_corners(ImagePoint::new(10.0, 20.0), ImagePoint::new(4.0, 6.0));
        assert_eq!(r, ImageRect { x: 4.0, y: 6.0, width: 6.0, height: 14.0 });
    }

    #[test]
    fn contains_and_center() {
        let r = ImageRect { x: 0.0, y: 0.0, width: 10.0, height: 4.0 };
        assert!(r.contains(ImagePoint::new(5.0, 2.0)));
        assert!(!r.contains(ImagePoint::new(11.0, 2.0)));
        assert_eq!(r.center(), ImagePoint::new(5.0, 2.0));
    }

    #[test]
    fn intersects_overlapping_and_disjoint() {
        let a = ImageRect { x: 0.0, y: 0.0, width: 10.0, height: 10.0 };
        let b = ImageRect { x: 5.0, y: 5.0, width: 10.0, height: 10.0 };
        let c = ImageRect { x: 20.0, y: 20.0, width: 5.0, height: 5.0 };
        assert!(a.intersects(&b));
        assert!(!a.intersects(&c));
    }

    #[test]
    fn point_clamp_to_image_bounds() {
        assert_eq!(
            ImagePoint::new(-5.0, 900.0).clamp_to(100, 200),
            ImagePoint::new(0.0, 200.0)
        );
    }

    #[test]
    fn rect_clamp_to_keeps_size_when_inside_and_clips_when_outside() {
        let inside = ImageRect { x: 5.0, y: 5.0, width: 10.0, height: 10.0 };
        assert_eq!(inside.clamp_to(100, 100), inside);
        let overflow = ImageRect { x: 95.0, y: 95.0, width: 10.0, height: 10.0 };
        let clipped = overflow.clamp_to(100, 100);
        assert_eq!(clipped, ImageRect { x: 95.0, y: 95.0, width: 5.0, height: 5.0 });
    }

    #[test]
    fn zero_area_rect_is_empty() {
        assert!(ImageRect { x: 0.0, y: 0.0, width: 0.5, height: 10.0 }.is_empty());
        assert!(!ImageRect { x: 0.0, y: 0.0, width: 1.0, height: 1.0 }.is_empty());
    }

    #[test]
    fn distance_is_euclidean() {
        assert_eq!(ImagePoint::new(0.0, 0.0).distance(ImagePoint::new(3.0, 4.0)), 5.0);
    }
}
```

- [ ] **Step 2: Run to verify failure**

Run: `rtk cargo test -p rollshot-image-document`
Expected: FAIL — `ImagePoint`/`ImageRect` not defined.

- [ ] **Step 3: Implement** (top of `geometry.rs`)

```rust
//! Image-space geometry. All coordinates are full-resolution image pixels,
//! independent of any viewport zoom or scroll.

/// A point in full-resolution image coordinates.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ImagePoint {
    pub x: f32,
    pub y: f32,
}

impl ImagePoint {
    pub const fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }

    pub fn distance(self, other: Self) -> f32 {
        ((self.x - other.x).powi(2) + (self.y - other.y).powi(2)).sqrt()
    }

    /// Clamp into the bounds of a `width × height` image.
    pub fn clamp_to(self, width: u32, height: u32) -> Self {
        Self {
            x: self.x.clamp(0.0, width as f32),
            y: self.y.clamp(0.0, height as f32),
        }
    }
}

/// An axis-aligned rectangle in full-resolution image coordinates.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ImageRect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl ImageRect {
    /// Normalized rect spanning two corners (handles inverted drags).
    pub fn from_corners(a: ImagePoint, b: ImagePoint) -> Self {
        let x = a.x.min(b.x);
        let y = a.y.min(b.y);
        Self {
            x,
            y,
            width: (a.x - b.x).abs(),
            height: (a.y - b.y).abs(),
        }
    }

    pub fn contains(&self, p: ImagePoint) -> bool {
        p.x >= self.x && p.x <= self.x + self.width && p.y >= self.y && p.y <= self.y + self.height
    }

    pub fn intersects(&self, other: &ImageRect) -> bool {
        self.x < other.x + other.width
            && other.x < self.x + self.width
            && self.y < other.y + other.height
            && other.y < self.y + self.height
    }

    pub fn center(&self) -> ImagePoint {
        ImagePoint::new(self.x + self.width / 2.0, self.y + self.height / 2.0)
    }

    /// Intersect with the image bounds, clipping overflow.
    pub fn clamp_to(self, width: u32, height: u32) -> Self {
        let x0 = self.x.clamp(0.0, width as f32);
        let y0 = self.y.clamp(0.0, height as f32);
        let x1 = (self.x + self.width).clamp(0.0, width as f32);
        let y1 = (self.y + self.height).clamp(0.0, height as f32);
        Self {
            x: x0,
            y: y0,
            width: (x1 - x0).max(0.0),
            height: (y1 - y0).max(0.0),
        }
    }

    /// Sub-pixel rects are treated as zero-area (spec §6: not committed).
    pub fn is_empty(&self) -> bool {
        self.width < 1.0 || self.height < 1.0
    }

    pub fn expanded(&self, margin: f32) -> Self {
        Self {
            x: self.x - margin,
            y: self.y - margin,
            width: self.width + margin * 2.0,
            height: self.height + margin * 2.0,
        }
    }
}

/// An 8-bit sRGB color with alpha, the form both `image::Rgba` and
/// `iced::Color::from_rgba8` can consume.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rgba8 {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Rgba8 {
    pub const fn new(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self { r, g, b, a }
    }
}
```

Uncomment `pub use geometry::{ImagePoint, ImageRect, Rgba8};` in `lib.rs`.

- [ ] **Step 4: Run tests**

Run: `rtk cargo test -p rollshot-image-document`
Expected: PASS (7 tests).

- [ ] **Step 5: Commit**

```bash
rtk git add crates/rollshot-image-document
rtk git commit -m "feat(image-document): image-space geometry types"
```

---

### Task 3: Annotation model and visual style constants

**Files:**
- Modify: `crates/rollshot-image-document/src/annotation.rs`
- Modify: `crates/rollshot-image-document/src/style.rs`
- Modify: `crates/rollshot-image-document/src/lib.rs` (uncomment `pub use annotation::...`)

- [ ] **Step 1: Write failing tests** (bottom of `annotation.rs`)

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::{ImagePoint, ImageRect};

    #[test]
    fn anchor_is_bubble_for_number_position_for_text_topleft_for_redaction() {
        let n = Annotation::NumberCallout {
            id: AnnotationId(1),
            number: 1,
            tip: ImagePoint::new(5.0, 5.0),
            bubble: ImagePoint::new(40.0, 60.0),
        };
        assert_eq!(n.anchor(), ImagePoint::new(40.0, 60.0));

        let t = Annotation::TextNote {
            id: AnnotationId(2),
            position: ImagePoint::new(7.0, 8.0),
            text: "hi".to_string(),
        };
        assert_eq!(t.anchor(), ImagePoint::new(7.0, 8.0));

        let r = Annotation::OpaqueRedaction {
            id: AnnotationId(3),
            bounds: ImageRect { x: 1.0, y: 2.0, width: 3.0, height: 4.0 },
        };
        assert_eq!(r.anchor(), ImagePoint::new(1.0, 2.0));
    }

    #[test]
    fn id_accessor_returns_each_variant_id() {
        let r = Annotation::OpaqueRedaction {
            id: AnnotationId(9),
            bounds: ImageRect { x: 0.0, y: 0.0, width: 2.0, height: 2.0 },
        };
        assert_eq!(r.id(), AnnotationId(9));
    }
}
```

- [ ] **Step 2: Run to verify failure**

Run: `rtk cargo test -p rollshot-image-document`
Expected: FAIL — types not defined.

- [ ] **Step 3: Implement `annotation.rs`**

```rust
//! The annotation graph. Geometry is stored in full-resolution image
//! coordinates (spec §6); IDs are stable across undo/redo.

use crate::geometry::{ImagePoint, ImageRect};

/// Stable annotation identity, never reused within a document.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct AnnotationId(pub u64);

#[derive(Debug, Clone, PartialEq)]
pub enum Annotation {
    NumberCallout {
        id: AnnotationId,
        number: u32,
        /// The pointed-at location (leader tip).
        tip: ImagePoint,
        /// The number bubble center. Coincident with `tip` for a stamp.
        bubble: ImagePoint,
    },
    TextNote {
        id: AnnotationId,
        /// Top-left of the backing plate.
        position: ImagePoint,
        text: String,
    },
    OpaqueRedaction {
        id: AnnotationId,
        bounds: ImageRect,
    },
}

impl Annotation {
    pub fn id(&self) -> AnnotationId {
        match self {
            Annotation::NumberCallout { id, .. }
            | Annotation::TextNote { id, .. }
            | Annotation::OpaqueRedaction { id, .. } => *id,
        }
    }

    /// Reading-order anchor used for Navigator ordering (spec §8.2).
    pub fn anchor(&self) -> ImagePoint {
        match self {
            Annotation::NumberCallout { bubble, .. } => *bubble,
            Annotation::TextNote { position, .. } => *position,
            Annotation::OpaqueRedaction { bounds, .. } => ImagePoint::new(bounds.x, bounds.y),
        }
    }
}
```

- [ ] **Step 4: Implement `style.rs` — the reviewed visual-defaults deliverable**

These constants ARE the spec §6 "fixed product defaults" the product review required as an explicit deliverable. Values derive from the mark-shot reference geometry (`learn-projects/mark-shot/src/shot_window_annotation_painting.cpp:503`) adapted to fixed first-release sizes:

```rust
//! First-release visual defaults (spec §6 — reviewed product deliverable).
//! All sizes are full-resolution image pixels. The UI exposes no style
//! controls; these constants are the single source of annotation appearance
//! for BOTH the live overlay and flattened output.

use crate::geometry::Rgba8;

/// Callout accent (number bubble fill, leader triangle): #E5484D.
pub const ACCENT: Rgba8 = Rgba8::new(0xE5, 0x48, 0x4D, 0xFF);
pub const WHITE: Rgba8 = Rgba8::new(0xFF, 0xFF, 0xFF, 0xFF);

/// Number bubble radius.
pub const NUMBER_BUBBLE_RADIUS: f32 = 17.0;
/// White outline ring width around the bubble (contrast treatment so the
/// bubble reads on accent-colored content).
pub const NUMBER_BUBBLE_OUTLINE_WIDTH: f32 = 2.0;
/// Number label: bold white digits, shrink-to-fit below this size.
pub const NUMBER_FONT_PX: f32 = 20.0;
pub const NUMBER_FONT_MIN_PX: f32 = 9.0;
/// Label must fit within this multiple of the bubble radius.
pub const NUMBER_LABEL_MAX_WIDTH_FACTOR: f32 = 1.6;

/// Leader triangle half-width at its base.
pub const LEADER_HALF_WIDTH: f32 = 8.0;
/// Leader base center sits at this fraction of the radius from bubble center.
pub const LEADER_BASE_FACTOR: f32 = 0.82;
/// Below this separation (× radius) no leader is drawn — the callout is a
/// plain stamp (click-created).
pub const LEADER_MIN_SEPARATION_FACTOR: f32 = 0.45;

/// Text Note: white text on a dark backing plate for legibility over busy
/// screenshot content (square corners in the first release).
pub const TEXT_NOTE_FONT_PX: f32 = 18.0;
pub const TEXT_NOTE_TEXT_COLOR: Rgba8 = WHITE;
pub const TEXT_NOTE_PLATE: Rgba8 = Rgba8::new(0x11, 0x11, 0x11, 0xD9); // ~85% black
pub const TEXT_NOTE_PLATE_PADDING: f32 = 8.0;
/// Line height factor (matches iced's default relative line height).
pub const TEXT_LINE_HEIGHT: f32 = 1.3;

/// Opaque Redaction: solid black, replaces covered pixels (spec §9.4).
pub const REDACTION_FILL: Rgba8 = Rgba8::new(0x00, 0x00, 0x00, 0xFF);

/// Deterministic baseline fonts, vendored. cosmic-text falls back to system
/// fonts for glyphs these lack (e.g. CJK).
pub const FONT_REGULAR_BYTES: &[u8] = include_bytes!("../assets/fonts/DejaVuSans.ttf");
pub const FONT_BOLD_BYTES: &[u8] = include_bytes!("../assets/fonts/DejaVuSans-Bold.ttf");
/// Family name consumers use to select the vendored font (e.g. iced
/// `Font::with_name`).
pub const FONT_FAMILY_NAME: &str = "DejaVu Sans";
```

Uncomment `pub use annotation::{Annotation, AnnotationId};` in `lib.rs` (style is already `pub mod`).

- [ ] **Step 5: Run tests**

Run: `rtk cargo test -p rollshot-image-document`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
rtk git add crates/rollshot-image-document
rtk git commit -m "feat(image-document): annotation model and visual default constants"
```

---

### Task 4: Text measurement and rasterization (cosmic-text)

**Files:**
- Modify: `crates/rollshot-image-document/src/text.rs`
- Modify: `crates/rollshot-image-document/src/lib.rs` (uncomment `pub use text::measure_block;`)

- [ ] **Step 1: Write failing tests** (bottom of `text.rs`)

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::{ImagePoint, Rgba8};
    use image::RgbaImage;

    #[test]
    fn measure_is_positive_and_grows_with_text() {
        let (w1, h1) = measure_block("1", 20.0, true);
        let (w2, _) = measure_block("10", 20.0, true);
        assert!(w1 > 0.0 && h1 > 0.0);
        assert!(w2 > w1);
    }

    #[test]
    fn multiline_is_taller_and_width_is_max_line() {
        let (w1, h1) = measure_block("hello", 18.0, false);
        let (w2, h2) = measure_block("hello\nhi", 18.0, false);
        assert!(h2 > h1);
        assert!((w2 - w1).abs() < 1.0, "width should match the longest line");
    }

    #[test]
    fn draw_block_blends_pixels_into_image() {
        let mut img = RgbaImage::from_pixel(60, 40, image::Rgba([0, 0, 0, 255]));
        draw_block(
            &mut img,
            ImagePoint::new(2.0, 2.0),
            "Hi",
            18.0,
            false,
            Rgba8::new(255, 255, 255, 255),
        );
        let changed = img.pixels().filter(|p| p.0 != [0, 0, 0, 255]).count();
        assert!(changed > 10, "expected glyph pixels, got {changed}");
    }
}
```

- [ ] **Step 2: Run to verify failure**

Run: `rtk cargo test -p rollshot-image-document`
Expected: FAIL — functions not defined.

- [ ] **Step 3: Implement `text.rs`**

```rust
//! Text shaping, measurement, and rasterization via cosmic-text. The vendored
//! DejaVu fonts are the deterministic baseline; system fonts provide fallback
//! coverage (CJK etc.). Both the plate geometry (`shapes.rs`) and flattened
//! glyph raster use THIS module, so measured layout and drawn output agree.

use std::sync::{Mutex, OnceLock};

use cosmic_text::{Attrs, Buffer, Family, FontSystem, Metrics, Shaping, SwashCache, Weight};
use image::RgbaImage;

use crate::geometry::{ImagePoint, Rgba8};
use crate::raster::blend_px;
use crate::style;

struct TextSystem {
    fonts: FontSystem,
    cache: SwashCache,
}

fn system() -> &'static Mutex<TextSystem> {
    static SYSTEM: OnceLock<Mutex<TextSystem>> = OnceLock::new();
    SYSTEM.get_or_init(|| {
        let mut fonts = FontSystem::new();
        fonts.db_mut().load_font_data(style::FONT_REGULAR_BYTES.to_vec());
        fonts.db_mut().load_font_data(style::FONT_BOLD_BYTES.to_vec());
        Mutex::new(TextSystem { fonts, cache: SwashCache::new() })
    })
}

fn attrs(bold: bool) -> Attrs<'static> {
    let attrs = Attrs::new().family(Family::Name(style::FONT_FAMILY_NAME));
    if bold {
        attrs.weight(Weight::BOLD)
    } else {
        attrs
    }
}

fn shaped_buffer(fonts: &mut FontSystem, text: &str, px: f32) -> Buffer {
    let metrics = Metrics::new(px, px * style::TEXT_LINE_HEIGHT);
    let mut buffer = Buffer::new(fonts, metrics);
    buffer.set_size(fonts, None, None);
    buffer
}

/// Measure a text block (lines split on `\n`, no soft wrapping).
/// Returns `(max_line_width, total_height)` in image pixels.
pub fn measure_block(text: &str, px: f32, bold: bool) -> (f32, f32) {
    let mut sys = system().lock().expect("text system poisoned");
    let TextSystem { fonts, .. } = &mut *sys;
    let mut buffer = shaped_buffer(fonts, text, px);
    buffer.set_text(fonts, text, &attrs(bold), Shaping::Advanced, None);
    buffer.shape_until_scroll(fonts, false);

    let mut width: f32 = 0.0;
    let mut lines: usize = 0;
    for run in buffer.layout_runs() {
        width = width.max(run.line_w);
        lines += 1;
    }
    (width, lines.max(1) as f32 * px * style::TEXT_LINE_HEIGHT)
}

/// Rasterize a text block onto `img` with its top-left at `top_left`.
pub(crate) fn draw_block(
    img: &mut RgbaImage,
    top_left: ImagePoint,
    text: &str,
    px: f32,
    bold: bool,
    color: Rgba8,
) {
    let mut sys = system().lock().expect("text system poisoned");
    let TextSystem { fonts, cache } = &mut *sys;
    let mut buffer = shaped_buffer(fonts, text, px);
    buffer.set_text(fonts, text, &attrs(bold), Shaping::Advanced, None);
    buffer.shape_until_scroll(fonts, false);

    let base = cosmic_text::Color::rgba(color.r, color.g, color.b, color.a);
    let (ox, oy) = (top_left.x.round() as i32, top_left.y.round() as i32);
    buffer.draw(fonts, cache, base, |x, y, w, h, c| {
        let alpha = (c.0 >> 24) as u8;
        if alpha == 0 {
            return;
        }
        let px_color = Rgba8::new(c.r(), c.g(), c.b(), alpha);
        for dy in 0..h as i32 {
            for dx in 0..w as i32 {
                blend_px(img, ox + x + dx, oy + y + dy, px_color, 1.0);
            }
        }
    });
}
```

Note: `raster::blend_px` lands in Task 11. To keep this task self-contained and green, implement `blend_px` now in `raster.rs` (it is the foundation of that module anyway):

```rust
//! Minimal anti-aliased software rasterizer for flattened output.

use image::RgbaImage;

use crate::geometry::Rgba8;

/// Source-over blend of `color` at `coverage` (0..=1) into pixel (x, y).
/// Out-of-bounds coordinates are ignored.
pub(crate) fn blend_px(img: &mut RgbaImage, x: i32, y: i32, color: Rgba8, coverage: f32) {
    if x < 0 || y < 0 || x >= img.width() as i32 || y >= img.height() as i32 {
        return;
    }
    let a = (color.a as f32 / 255.0) * coverage.clamp(0.0, 1.0);
    if a <= 0.0 {
        return;
    }
    let dst = img.get_pixel_mut(x as u32, y as u32);
    let blend = |src: u8, dst: u8| -> u8 {
        (src as f32 * a + dst as f32 * (1.0 - a)).round() as u8
    };
    let out_a = a + (dst.0[3] as f32 / 255.0) * (1.0 - a);
    dst.0 = [
        blend(color.r, dst.0[0]),
        blend(color.g, dst.0[1]),
        blend(color.b, dst.0[2]),
        (out_a * 255.0).round() as u8,
    ];
}
```

If a cosmic-text 0.15 signature differs at compile time (e.g. `set_size` arity or `Color` channel accessors `c.r()/c.g()/c.b()`), fix mechanically against `~/.cargo/registry/src/*/cosmic-text-0.15.0/src/buffer.rs` — the call sequence (set_text → shape_until_scroll → layout_runs/draw) is verified.

- [ ] **Step 4: Run tests**

Run: `rtk cargo test -p rollshot-image-document`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
rtk git add crates/rollshot-image-document
rtk git commit -m "feat(image-document): cosmic-text measurement and glyph rasterization"
```

---

### Task 5: ImageDocument creation edits and number allocation

**Files:**
- Modify: `crates/rollshot-image-document/src/document.rs`
- Modify: `crates/rollshot-image-document/src/lib.rs` (uncomment `pub use document::...`)

- [ ] **Step 1: Write failing tests** (bottom of `document.rs`)

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::{ImagePoint, ImageRect};
    use image::{Rgba, RgbaImage};

    pub(crate) fn doc() -> ImageDocument {
        ImageDocument::new(RgbaImage::from_pixel(100, 200, Rgba([10, 20, 30, 255])))
    }

    #[test]
    fn new_document_is_empty_with_number_sequence_at_one() {
        let d = doc();
        assert!(d.annotations().is_empty());
        assert_eq!(d.next_number(), 1);
        assert!(!d.can_undo() && !d.can_redo());
    }

    #[test]
    fn add_number_callouts_allocates_sequential_numbers_and_unique_ids() {
        let mut d = doc();
        let a = d.add_number_callout(ImagePoint::new(1.0, 1.0), ImagePoint::new(1.0, 1.0));
        let b = d.add_number_callout(ImagePoint::new(2.0, 2.0), ImagePoint::new(9.0, 9.0));
        assert_ne!(a, b);
        let numbers: Vec<u32> = d
            .annotations()
            .iter()
            .map(|ann| match ann {
                Annotation::NumberCallout { number, .. } => *number,
                _ => panic!("expected number callout"),
            })
            .collect();
        assert_eq!(numbers, vec![1, 2]);
        assert_eq!(d.next_number(), 3);
    }

    #[test]
    fn add_text_note_rejects_whitespace_only_text() {
        let mut d = doc();
        assert_eq!(
            d.add_text_note(ImagePoint::new(5.0, 5.0), "   \n ".to_string()),
            Err(EditError::EmptyText)
        );
        assert!(d.annotations().is_empty());
        assert!(!d.can_undo(), "rejected edit must not enter history");
    }

    #[test]
    fn add_redaction_rejects_zero_area_after_clamp() {
        let mut d = doc();
        let zero = ImageRect { x: 5.0, y: 5.0, width: 0.4, height: 50.0 };
        assert_eq!(d.add_redaction(zero), Err(EditError::ZeroArea));
        // Entirely outside the image clamps to nothing.
        let outside = ImageRect { x: 500.0, y: 500.0, width: 50.0, height: 50.0 };
        assert_eq!(d.add_redaction(outside), Err(EditError::ZeroArea));
        assert!(d.annotations().is_empty());
    }

    #[test]
    fn add_clamps_geometry_into_image_bounds() {
        let mut d = doc();
        d.add_number_callout(ImagePoint::new(-10.0, 50.0), ImagePoint::new(150.0, 300.0));
        match &d.annotations()[0] {
            Annotation::NumberCallout { tip, bubble, .. } => {
                assert_eq!(*tip, ImagePoint::new(0.0, 50.0));
                assert_eq!(*bubble, ImagePoint::new(100.0, 200.0));
            }
            _ => panic!("expected number callout"),
        }
    }

    #[test]
    fn source_pixels_unchanged_by_edits() {
        let mut d = doc();
        let before = d.source().clone();
        d.add_number_callout(ImagePoint::new(1.0, 1.0), ImagePoint::new(2.0, 2.0));
        let _ = d.add_text_note(ImagePoint::new(5.0, 5.0), "note".to_string());
        let _ = d.add_redaction(ImageRect { x: 0.0, y: 0.0, width: 10.0, height: 10.0 });
        assert_eq!(d.source().as_raw(), before.as_raw());
    }

    #[test]
    fn state_id_changes_on_every_commit() {
        let mut d = doc();
        let s0 = d.state_id();
        d.add_number_callout(ImagePoint::new(1.0, 1.0), ImagePoint::new(1.0, 1.0));
        let s1 = d.state_id();
        assert_ne!(s0, s1);
        let _ = d.add_text_note(ImagePoint::new(5.0, 5.0), "x".to_string());
        assert_ne!(d.state_id(), s1);
    }
}
```

- [ ] **Step 2: Run to verify failure**

Run: `rtk cargo test -p rollshot-image-document`
Expected: FAIL — `ImageDocument` not defined.

- [ ] **Step 3: Implement `document.rs` (creation half)**

```rust
//! The non-destructive image document: immutable source, annotation graph,
//! number sequence, and snapshot-based history (spec §6, §10).

use std::collections::VecDeque;

use image::RgbaImage;

use crate::annotation::{Annotation, AnnotationId};
use crate::geometry::{ImagePoint, ImageRect};
use crate::hit::Hit;
use crate::navigator::NavigatorItem;

/// Maximum undo entries (spec §10).
pub const HISTORY_LIMIT: usize = 100;

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum EditError {
    #[error("text notes must contain non-whitespace text")]
    EmptyText,
    #[error("redactions must cover at least one pixel")]
    ZeroArea,
    #[error("annotation does not exist")]
    UnknownAnnotation,
    #[error("operation does not apply to this annotation kind")]
    WrongKind,
}

/// One restorable history state (mark-shot pattern: graph + counters).
#[derive(Debug, Clone)]
struct Snapshot {
    annotations: Vec<Annotation>,
    next_number: u32,
    state_id: u64,
}

pub struct ImageDocument {
    source: RgbaImage,
    annotations: Vec<Annotation>,
    next_number: u32,
    next_id: u64,
    /// Identity of the current document state; restored by undo/redo so the
    /// editor can compare against a saved marker (dirty tracking).
    state_id: u64,
    next_state_id: u64,
    undo_stack: VecDeque<Snapshot>,
    redo_stack: Vec<Snapshot>,
}

impl ImageDocument {
    pub fn new(source: RgbaImage) -> Self {
        Self {
            source,
            annotations: Vec::new(),
            next_number: 1,
            next_id: 1,
            state_id: 0,
            next_state_id: 0,
            undo_stack: VecDeque::new(),
            redo_stack: Vec::new(),
        }
    }

    pub fn source(&self) -> &RgbaImage {
        &self.source
    }

    pub fn annotations(&self) -> &[Annotation] {
        &self.annotations
    }

    pub fn annotation(&self, id: AnnotationId) -> Option<&Annotation> {
        self.annotations.iter().find(|a| a.id() == id)
    }

    pub fn next_number(&self) -> u32 {
        self.next_number
    }

    pub fn state_id(&self) -> u64 {
        self.state_id
    }

    pub fn can_undo(&self) -> bool {
        !self.undo_stack.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo_stack.is_empty()
    }

    fn snapshot(&self) -> Snapshot {
        Snapshot {
            annotations: self.annotations.clone(),
            next_number: self.next_number,
            state_id: self.state_id,
        }
    }

    /// Record `before` as an undo entry and stamp a fresh state id.
    /// Called exactly once per completed semantic edit (spec §10).
    fn commit(&mut self, before: Snapshot) {
        self.undo_stack.push_back(before);
        if self.undo_stack.len() > HISTORY_LIMIT {
            self.undo_stack.pop_front();
        }
        self.redo_stack.clear();
        self.next_state_id += 1;
        self.state_id = self.next_state_id;
    }

    fn allocate_id(&mut self) -> AnnotationId {
        let id = AnnotationId(self.next_id);
        self.next_id += 1;
        id
    }

    pub fn add_number_callout(&mut self, tip: ImagePoint, bubble: ImagePoint) -> AnnotationId {
        let before = self.snapshot();
        let (w, h) = self.source.dimensions();
        let id = self.allocate_id();
        let number = self.next_number;
        self.next_number += 1;
        self.annotations.push(Annotation::NumberCallout {
            id,
            number,
            tip: tip.clamp_to(w, h),
            bubble: bubble.clamp_to(w, h),
        });
        self.commit(before);
        id
    }

    pub fn add_text_note(
        &mut self,
        position: ImagePoint,
        text: String,
    ) -> Result<AnnotationId, EditError> {
        if text.trim().is_empty() {
            return Err(EditError::EmptyText);
        }
        let before = self.snapshot();
        let (w, h) = self.source.dimensions();
        let id = self.allocate_id();
        self.annotations.push(Annotation::TextNote {
            id,
            position: position.clamp_to(w, h),
            text,
        });
        self.commit(before);
        Ok(id)
    }

    pub fn add_redaction(&mut self, bounds: ImageRect) -> Result<AnnotationId, EditError> {
        let (w, h) = self.source.dimensions();
        let clamped = bounds.clamp_to(w, h);
        if clamped.is_empty() {
            return Err(EditError::ZeroArea);
        }
        let before = self.snapshot();
        let id = self.allocate_id();
        self.annotations
            .push(Annotation::OpaqueRedaction { id, bounds: clamped });
        self.commit(before);
        Ok(id)
    }
}
```

Add `thiserror = { workspace = true }` is already in Cargo.toml (Task 1). The `hit::Hit` / `navigator::NavigatorItem` imports are used by methods added in Tasks 9–10; if the compiler flags them as unused now, add them in those tasks instead. Uncomment `pub use document::{EditError, ImageDocument, HISTORY_LIMIT};` in `lib.rs`.

- [ ] **Step 4: Run tests**

Run: `rtk cargo test -p rollshot-image-document`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
rtk git add crates/rollshot-image-document
rtk git commit -m "feat(image-document): document creation edits and number allocation"
```

---

### Task 6: Undo/redo history

**Files:**
- Modify: `crates/rollshot-image-document/src/document.rs`

- [ ] **Step 1: Write failing tests** (append inside `mod tests`)

```rust
    #[test]
    fn undo_redo_restore_annotations_sequence_and_state_id() {
        let mut d = doc();
        let s0 = d.state_id();
        d.add_number_callout(ImagePoint::new(1.0, 1.0), ImagePoint::new(1.0, 1.0));
        let s1 = d.state_id();
        d.add_number_callout(ImagePoint::new(2.0, 2.0), ImagePoint::new(2.0, 2.0));

        assert!(d.undo());
        assert_eq!(d.annotations().len(), 1);
        assert_eq!(d.next_number(), 2, "sequence follows undo (spec §6)");
        assert_eq!(d.state_id(), s1);

        assert!(d.undo());
        assert!(d.annotations().is_empty());
        assert_eq!(d.next_number(), 1);
        assert_eq!(d.state_id(), s0);
        assert!(!d.undo(), "nothing left to undo");

        assert!(d.redo());
        assert_eq!(d.annotations().len(), 1);
        assert_eq!(d.next_number(), 2);
        assert_eq!(d.state_id(), s1);
    }

    #[test]
    fn new_edit_after_undo_clears_redo() {
        let mut d = doc();
        d.add_number_callout(ImagePoint::new(1.0, 1.0), ImagePoint::new(1.0, 1.0));
        d.add_number_callout(ImagePoint::new(2.0, 2.0), ImagePoint::new(2.0, 2.0));
        assert!(d.undo());
        assert!(d.can_redo());
        d.add_number_callout(ImagePoint::new(3.0, 3.0), ImagePoint::new(3.0, 3.0));
        assert!(!d.can_redo(), "spec §10: new edit clears redo");
    }

    #[test]
    fn history_caps_at_limit_dropping_oldest() {
        let mut d = doc();
        for i in 0..(HISTORY_LIMIT + 10) {
            d.add_number_callout(
                ImagePoint::new(i as f32 % 90.0, 1.0),
                ImagePoint::new(i as f32 % 90.0, 1.0),
            );
        }
        let mut undone = 0;
        while d.undo() {
            undone += 1;
        }
        assert_eq!(undone, HISTORY_LIMIT);
        assert_eq!(d.annotations().len(), 10, "oldest 10 edits fell off the stack");
    }

    #[test]
    fn ids_stay_stable_across_undo_redo_and_are_never_reused() {
        let mut d = doc();
        let first = d.add_number_callout(ImagePoint::new(1.0, 1.0), ImagePoint::new(1.0, 1.0));
        assert!(d.undo());
        let second = d.add_number_callout(ImagePoint::new(2.0, 2.0), ImagePoint::new(2.0, 2.0));
        assert_ne!(first, second, "ids are never reused after undo");
        assert!(d.undo());
        assert!(d.redo());
        assert_eq!(d.annotations()[0].id(), second, "redo restores the same id");
    }
```

- [ ] **Step 2: Run to verify failure**

Run: `rtk cargo test -p rollshot-image-document`
Expected: FAIL — `undo`/`redo` not defined.

- [ ] **Step 3: Implement** (append to `impl ImageDocument`)

```rust
    fn restore(&mut self, snapshot: Snapshot) {
        self.annotations = snapshot.annotations;
        self.next_number = snapshot.next_number;
        self.state_id = snapshot.state_id;
    }

    /// Returns `false` when there is nothing to undo.
    pub fn undo(&mut self) -> bool {
        let Some(previous) = self.undo_stack.pop_back() else {
            return false;
        };
        self.redo_stack.push(self.snapshot());
        self.restore(previous);
        true
    }

    /// Returns `false` when there is nothing to redo.
    pub fn redo(&mut self) -> bool {
        let Some(next) = self.redo_stack.pop() else {
            return false;
        };
        self.undo_stack.push_back(self.snapshot());
        self.restore(next);
        true
    }
```

Note `next_id` is intentionally NOT in snapshots: IDs are never reused, so an annotation created after an undo gets a fresh ID and Navigator synchronization stays predictable (spec §10).

- [ ] **Step 4: Run tests**

Run: `rtk cargo test -p rollshot-image-document`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
rtk git add crates/rollshot-image-document
rtk git commit -m "feat(image-document): snapshot-based undo/redo with 100-entry cap"
```

---

### Task 7: Geometry setters, text edit, and delete with compact renumbering (D1)

**Files:**
- Modify: `crates/rollshot-image-document/src/document.rs`

- [ ] **Step 1: Write failing tests** (append inside `mod tests`)

```rust
    #[test]
    fn setters_update_geometry_and_are_undoable() {
        let mut d = doc();
        let id = d.add_number_callout(ImagePoint::new(5.0, 5.0), ImagePoint::new(5.0, 5.0));
        d.set_number_points(id, ImagePoint::new(10.0, 10.0), ImagePoint::new(40.0, 40.0))
            .unwrap();
        match d.annotation(id).unwrap() {
            Annotation::NumberCallout { tip, bubble, .. } => {
                assert_eq!(*tip, ImagePoint::new(10.0, 10.0));
                assert_eq!(*bubble, ImagePoint::new(40.0, 40.0));
            }
            _ => panic!(),
        }
        assert!(d.undo());
        match d.annotation(id).unwrap() {
            Annotation::NumberCallout { tip, .. } => assert_eq!(*tip, ImagePoint::new(5.0, 5.0)),
            _ => panic!(),
        }
    }

    #[test]
    fn unchanged_setter_is_a_no_op_without_history_entry() {
        let mut d = doc();
        let id = d.add_number_callout(ImagePoint::new(5.0, 5.0), ImagePoint::new(6.0, 6.0));
        let s = d.state_id();
        d.set_number_points(id, ImagePoint::new(5.0, 5.0), ImagePoint::new(6.0, 6.0))
            .unwrap();
        assert_eq!(d.state_id(), s, "no-op edit must not commit");
    }

    #[test]
    fn set_text_replaces_content_and_rejects_empty() {
        let mut d = doc();
        let id = d.add_text_note(ImagePoint::new(5.0, 5.0), "old".to_string()).unwrap();
        d.set_text(id, "new".to_string()).unwrap();
        match d.annotation(id).unwrap() {
            Annotation::TextNote { text, .. } => assert_eq!(text, "new"),
            _ => panic!(),
        }
        assert_eq!(d.set_text(id, "  ".to_string()), Err(EditError::EmptyText));
    }

    #[test]
    fn wrong_kind_and_unknown_id_are_rejected() {
        let mut d = doc();
        let id = d.add_text_note(ImagePoint::new(5.0, 5.0), "x".to_string()).unwrap();
        assert_eq!(
            d.set_number_points(id, ImagePoint::new(0.0, 0.0), ImagePoint::new(0.0, 0.0)),
            Err(EditError::WrongKind)
        );
        assert_eq!(
            d.delete_annotation(AnnotationId(999)),
            Err(EditError::UnknownAnnotation)
        );
    }

    #[test]
    fn set_redaction_bounds_resizes_and_rejects_zero_area() {
        let mut d = doc();
        let id = d
            .add_redaction(ImageRect { x: 1.0, y: 1.0, width: 10.0, height: 10.0 })
            .unwrap();
        d.set_redaction_bounds(id, ImageRect { x: 2.0, y: 2.0, width: 20.0, height: 5.0 })
            .unwrap();
        assert_eq!(
            d.set_redaction_bounds(id, ImageRect { x: 2.0, y: 2.0, width: 0.1, height: 5.0 }),
            Err(EditError::ZeroArea)
        );
    }

    // -- D1: compact renumbering on delete ------------------------------------

    fn numbers(d: &ImageDocument) -> Vec<u32> {
        d.annotations()
            .iter()
            .filter_map(|a| match a {
                Annotation::NumberCallout { number, .. } => Some(*number),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn deleting_a_middle_callout_renumbers_compactly() {
        let mut d = doc();
        let _one = d.add_number_callout(ImagePoint::new(1.0, 1.0), ImagePoint::new(1.0, 1.0));
        let two = d.add_number_callout(ImagePoint::new(2.0, 2.0), ImagePoint::new(2.0, 2.0));
        let _three = d.add_number_callout(ImagePoint::new(3.0, 3.0), ImagePoint::new(3.0, 3.0));

        d.delete_annotation(two).unwrap();
        assert_eq!(numbers(&d), vec![1, 2], "1,2,3 minus #2 compacts to 1,2");
        assert_eq!(d.next_number(), 3, "next allocation is highest remaining + 1");
    }

    #[test]
    fn delete_then_create_allocates_highest_remaining_plus_one() {
        let mut d = doc();
        d.add_number_callout(ImagePoint::new(1.0, 1.0), ImagePoint::new(1.0, 1.0));
        let two = d.add_number_callout(ImagePoint::new(2.0, 2.0), ImagePoint::new(2.0, 2.0));
        d.add_number_callout(ImagePoint::new(3.0, 3.0), ImagePoint::new(3.0, 3.0));
        d.delete_annotation(two).unwrap();
        d.add_number_callout(ImagePoint::new(4.0, 4.0), ImagePoint::new(4.0, 4.0));
        assert_eq!(numbers(&d), vec![1, 2, 3]);
    }

    #[test]
    fn undo_of_delete_restores_exact_prior_numbering_in_one_step() {
        let mut d = doc();
        d.add_number_callout(ImagePoint::new(1.0, 1.0), ImagePoint::new(1.0, 1.0));
        let two = d.add_number_callout(ImagePoint::new(2.0, 2.0), ImagePoint::new(2.0, 2.0));
        d.add_number_callout(ImagePoint::new(3.0, 3.0), ImagePoint::new(3.0, 3.0));
        d.delete_annotation(two).unwrap();
        assert!(d.undo(), "delete + renumber is ONE history entry");
        assert_eq!(numbers(&d), vec![1, 2, 3]);
        assert_eq!(d.next_number(), 4);
        assert_eq!(d.annotations()[1].id(), two, "identity preserved through undo");
    }

    #[test]
    fn deleting_last_callout_resets_sequence_and_non_number_delete_does_not_renumber() {
        let mut d = doc();
        let n = d.add_number_callout(ImagePoint::new(1.0, 1.0), ImagePoint::new(1.0, 1.0));
        let t = d.add_text_note(ImagePoint::new(5.0, 5.0), "x".to_string()).unwrap();
        d.delete_annotation(t).unwrap();
        assert_eq!(numbers(&d), vec![1], "text delete leaves numbering alone");
        d.delete_annotation(n).unwrap();
        assert_eq!(d.next_number(), 1, "no callouts left → sequence restarts at 1");
    }
```

- [ ] **Step 2: Run to verify failure**

Run: `rtk cargo test -p rollshot-image-document`
Expected: FAIL — setters/delete not defined.

- [ ] **Step 3: Implement** (append to `impl ImageDocument`)

```rust
    fn annotation_index(&self, id: AnnotationId) -> Result<usize, EditError> {
        self.annotations
            .iter()
            .position(|a| a.id() == id)
            .ok_or(EditError::UnknownAnnotation)
    }

    pub fn set_number_points(
        &mut self,
        id: AnnotationId,
        tip: ImagePoint,
        bubble: ImagePoint,
    ) -> Result<(), EditError> {
        let (w, h) = self.source.dimensions();
        let (tip, bubble) = (tip.clamp_to(w, h), bubble.clamp_to(w, h));
        let index = self.annotation_index(id)?;
        let before = self.snapshot();
        match &mut self.annotations[index] {
            Annotation::NumberCallout { tip: t, bubble: b, .. } => {
                if *t == tip && *b == bubble {
                    return Ok(());
                }
                *t = tip;
                *b = bubble;
            }
            _ => return Err(EditError::WrongKind),
        }
        self.commit(before);
        Ok(())
    }

    pub fn set_text_position(
        &mut self,
        id: AnnotationId,
        position: ImagePoint,
    ) -> Result<(), EditError> {
        let (w, h) = self.source.dimensions();
        let position = position.clamp_to(w, h);
        let index = self.annotation_index(id)?;
        let before = self.snapshot();
        match &mut self.annotations[index] {
            Annotation::TextNote { position: p, .. } => {
                if *p == position {
                    return Ok(());
                }
                *p = position;
            }
            _ => return Err(EditError::WrongKind),
        }
        self.commit(before);
        Ok(())
    }

    pub fn set_text(&mut self, id: AnnotationId, text: String) -> Result<(), EditError> {
        if text.trim().is_empty() {
            return Err(EditError::EmptyText);
        }
        let index = self.annotation_index(id)?;
        let before = self.snapshot();
        match &mut self.annotations[index] {
            Annotation::TextNote { text: t, .. } => {
                if *t == text {
                    return Ok(());
                }
                *t = text;
            }
            _ => return Err(EditError::WrongKind),
        }
        self.commit(before);
        Ok(())
    }

    pub fn set_redaction_bounds(
        &mut self,
        id: AnnotationId,
        bounds: ImageRect,
    ) -> Result<(), EditError> {
        let (w, h) = self.source.dimensions();
        let clamped = bounds.clamp_to(w, h);
        if clamped.is_empty() {
            return Err(EditError::ZeroArea);
        }
        let index = self.annotation_index(id)?;
        let before = self.snapshot();
        match &mut self.annotations[index] {
            Annotation::OpaqueRedaction { bounds: b, .. } => {
                if *b == clamped {
                    return Ok(());
                }
                *b = clamped;
            }
            _ => return Err(EditError::WrongKind),
        }
        self.commit(before);
        Ok(())
    }

    /// Delete an annotation. Deleting a Number Callout compactly renumbers
    /// the remaining callouts preserving relative order; the deletion and its
    /// renumbering form ONE history entry (spec §9.2, decision D1).
    pub fn delete_annotation(&mut self, id: AnnotationId) -> Result<(), EditError> {
        let index = self.annotation_index(id)?;
        let before = self.snapshot();
        let removed = self.annotations.remove(index);
        if matches!(removed, Annotation::NumberCallout { .. }) {
            self.renumber_compactly();
        }
        self.commit(before);
        Ok(())
    }

    /// Reassign callout numbers to 1..=n preserving current relative order;
    /// next allocation becomes n + 1.
    fn renumber_compactly(&mut self) {
        let mut callout_indices: Vec<usize> = self
            .annotations
            .iter()
            .enumerate()
            .filter(|(_, a)| matches!(a, Annotation::NumberCallout { .. }))
            .map(|(i, _)| i)
            .collect();
        callout_indices.sort_by_key(|&i| match &self.annotations[i] {
            Annotation::NumberCallout { number, .. } => *number,
            _ => unreachable!(),
        });
        for (new_number, &i) in callout_indices.iter().enumerate() {
            if let Annotation::NumberCallout { number, .. } = &mut self.annotations[i] {
                *number = new_number as u32 + 1;
            }
        }
        self.next_number = callout_indices.len() as u32 + 1;
    }
```

- [ ] **Step 4: Run tests**

Run: `rtk cargo test -p rollshot-image-document`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
rtk git add crates/rollshot-image-document
rtk git commit -m "feat(image-document): setters and delete with compact renumbering (D1)"
```

---

### Task 8: Shared render-shape model

This is the divergence killer (spec §11.2): every geometry decision — plate size from measured text, bubble/leader math, label sizing — is made HERE once. Flattening rasterizes these shapes; the iced canvas maps the same shapes to vector calls.

**Files:**
- Modify: `crates/rollshot-image-document/src/shapes.rs`
- Modify: `crates/rollshot-image-document/src/lib.rs` (uncomment `pub use shapes::...`)

- [ ] **Step 1: Write failing tests** (bottom of `shapes.rs`)

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::annotation::{Annotation, AnnotationId};
    use crate::geometry::{ImagePoint, ImageRect};
    use crate::style;

    fn number(tip: ImagePoint, bubble: ImagePoint) -> Annotation {
        Annotation::NumberCallout { id: AnnotationId(1), number: 3, tip, bubble }
    }

    #[test]
    fn coincident_callout_has_no_leader_triangle() {
        let p = ImagePoint::new(50.0, 50.0);
        let shapes = annotation_shapes(&number(p, p));
        assert!(!shapes.iter().any(|s| matches!(s, RenderShape::Triangle { .. })));
        assert!(shapes.iter().any(|s| matches!(s, RenderShape::Circle { .. })));
        assert!(shapes.iter().any(
            |s| matches!(s, RenderShape::Label { content, bold: true, .. } if content == "3")
        ));
    }

    #[test]
    fn separated_callout_has_leader_reaching_the_tip() {
        let tip = ImagePoint::new(10.0, 10.0);
        let bubble = ImagePoint::new(100.0, 10.0);
        let shapes = annotation_shapes(&number(tip, bubble));
        let triangle = shapes
            .iter()
            .find_map(|s| match s {
                RenderShape::Triangle { points, .. } => Some(points),
                _ => None,
            })
            .expect("separated callout draws a leader");
        assert_eq!(triangle[0], tip, "triangle apex is the tip");
    }

    #[test]
    fn text_plate_wraps_measured_text_with_padding() {
        let pos = ImagePoint::new(20.0, 30.0);
        let plate = text_plate_rect(pos, "hello");
        let (w, h) = crate::text::measure_block("hello", style::TEXT_NOTE_FONT_PX, false);
        assert_eq!(plate.x, pos.x);
        assert_eq!(plate.y, pos.y);
        assert!((plate.width - (w + style::TEXT_NOTE_PLATE_PADDING * 2.0)).abs() < 0.01);
        assert!((plate.height - (h + style::TEXT_NOTE_PLATE_PADDING * 2.0)).abs() < 0.01);
    }

    #[test]
    fn text_note_shapes_are_plate_then_label() {
        let note = Annotation::TextNote {
            id: AnnotationId(2),
            position: ImagePoint::new(20.0, 30.0),
            text: "hello".to_string(),
        };
        let shapes = annotation_shapes(&note);
        assert!(matches!(shapes[0], RenderShape::Rect { .. }));
        match &shapes[1] {
            RenderShape::Label { anchor, anchor_kind, bold, .. } => {
                assert_eq!(*anchor_kind, TextAnchor::TopLeft);
                assert!(!bold);
                assert_eq!(
                    *anchor,
                    ImagePoint::new(
                        20.0 + style::TEXT_NOTE_PLATE_PADDING,
                        30.0 + style::TEXT_NOTE_PLATE_PADDING
                    )
                );
            }
            other => panic!("expected label, got {other:?}"),
        }
    }

    #[test]
    fn bounds_cover_bubble_tip_plate_and_redaction() {
        let n = number(ImagePoint::new(10.0, 10.0), ImagePoint::new(100.0, 100.0));
        let b = annotation_bounds(&n);
        assert!(b.contains(ImagePoint::new(10.0, 10.0)));
        assert!(b.contains(ImagePoint::new(100.0 + style::NUMBER_BUBBLE_RADIUS - 1.0, 100.0)));

        let r = Annotation::OpaqueRedaction {
            id: AnnotationId(3),
            bounds: ImageRect { x: 5.0, y: 6.0, width: 7.0, height: 8.0 },
        };
        assert_eq!(annotation_bounds(&r), ImageRect { x: 5.0, y: 6.0, width: 7.0, height: 8.0 });
    }

    #[test]
    fn long_number_labels_shrink_to_fit() {
        let small = number_label_px("3");
        let large = number_label_px("888");
        assert_eq!(small, style::NUMBER_FONT_PX);
        assert!(large < small, "3-digit labels shrink to stay inside the bubble");
        assert!(large >= style::NUMBER_FONT_MIN_PX);
    }
}
```

- [ ] **Step 2: Run to verify failure**

Run: `rtk cargo test -p rollshot-image-document`
Expected: FAIL — shapes API not defined.

- [ ] **Step 3: Implement `shapes.rs`**

```rust
//! The shared render-shape model: the single source of annotation geometry
//! for BOTH flattened output (raster.rs/flatten.rs) and any live overlay
//! renderer. Leader geometry follows the mark-shot reference
//! (learn-projects/mark-shot/src/shot_window_annotation_painting.cpp:503).

use crate::annotation::Annotation;
use crate::geometry::{ImagePoint, ImageRect, Rgba8};
use crate::style;
use crate::text::measure_block;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextAnchor {
    /// `anchor` is the top-left of the laid-out block.
    TopLeft,
    /// `anchor` is the visual center of the laid-out block.
    Center,
}

/// A framework-neutral drawing primitive in image coordinates.
#[derive(Debug, Clone, PartialEq)]
pub enum RenderShape {
    Rect {
        rect: ImageRect,
        color: Rgba8,
    },
    Circle {
        center: ImagePoint,
        radius: f32,
        fill: Rgba8,
        outline_width: f32,
        outline: Rgba8,
    },
    Triangle {
        points: [ImagePoint; 3],
        color: Rgba8,
    },
    Label {
        anchor: ImagePoint,
        anchor_kind: TextAnchor,
        content: String,
        px: f32,
        bold: bool,
        color: Rgba8,
    },
}

/// Font size for a number label, shrunk until it fits the bubble.
pub fn number_label_px(label: &str) -> f32 {
    let max_width = style::NUMBER_BUBBLE_RADIUS * style::NUMBER_LABEL_MAX_WIDTH_FACTOR;
    let mut px = style::NUMBER_FONT_PX;
    while px > style::NUMBER_FONT_MIN_PX {
        let (w, _) = measure_block(label, px, true);
        if w <= max_width {
            break;
        }
        px -= 1.0;
    }
    px
}

/// Leader triangle from bubble edge to tip, or `None` when the separation is
/// too small to read (the callout renders as a plain stamp).
pub(crate) fn leader_triangle(tip: ImagePoint, bubble: ImagePoint) -> Option<[ImagePoint; 3]> {
    let radius = style::NUMBER_BUBBLE_RADIUS;
    let length = bubble.distance(tip);
    if length <= radius * style::LEADER_MIN_SEPARATION_FACTOR {
        return None;
    }
    let dir = ((tip.x - bubble.x) / length, (tip.y - bubble.y) / length);
    let normal = (-dir.1, dir.0);
    let base = ImagePoint::new(
        bubble.x + dir.0 * radius * style::LEADER_BASE_FACTOR,
        bubble.y + dir.1 * radius * style::LEADER_BASE_FACTOR,
    );
    let hw = style::LEADER_HALF_WIDTH;
    Some([
        tip,
        ImagePoint::new(base.x + normal.0 * hw, base.y + normal.1 * hw),
        ImagePoint::new(base.x - normal.0 * hw, base.y - normal.1 * hw),
    ])
}

/// Backing plate for a text note positioned at `position` (its top-left).
pub fn text_plate_rect(position: ImagePoint, text: &str) -> ImageRect {
    let (w, h) = measure_block(text, style::TEXT_NOTE_FONT_PX, false);
    let pad = style::TEXT_NOTE_PLATE_PADDING;
    ImageRect {
        x: position.x,
        y: position.y,
        width: w + pad * 2.0,
        height: h + pad * 2.0,
    }
}

/// Drawing primitives for one committed annotation, in paint order.
/// Flattening never includes selection handles, hover effects, or drafts
/// (spec §6) — those are editor concerns and never enter this model.
pub fn annotation_shapes(annotation: &Annotation) -> Vec<RenderShape> {
    match annotation {
        Annotation::NumberCallout { number, tip, bubble, .. } => {
            let mut shapes = Vec::with_capacity(3);
            if let Some(points) = leader_triangle(*tip, *bubble) {
                shapes.push(RenderShape::Triangle { points, color: style::ACCENT });
            }
            shapes.push(RenderShape::Circle {
                center: *bubble,
                radius: style::NUMBER_BUBBLE_RADIUS,
                fill: style::ACCENT,
                outline_width: style::NUMBER_BUBBLE_OUTLINE_WIDTH,
                outline: style::WHITE,
            });
            let label = number.to_string();
            let px = number_label_px(&label);
            shapes.push(RenderShape::Label {
                anchor: *bubble,
                anchor_kind: TextAnchor::Center,
                content: label,
                px,
                bold: true,
                color: style::WHITE,
            });
            shapes
        }
        Annotation::TextNote { position, text, .. } => {
            let pad = style::TEXT_NOTE_PLATE_PADDING;
            vec![
                RenderShape::Rect {
                    rect: text_plate_rect(*position, text),
                    color: style::TEXT_NOTE_PLATE,
                },
                RenderShape::Label {
                    anchor: ImagePoint::new(position.x + pad, position.y + pad),
                    anchor_kind: TextAnchor::TopLeft,
                    content: text.clone(),
                    px: style::TEXT_NOTE_FONT_PX,
                    bold: false,
                    color: style::TEXT_NOTE_TEXT_COLOR,
                },
            ]
        }
        Annotation::OpaqueRedaction { bounds, .. } => vec![RenderShape::Rect {
            rect: *bounds,
            color: style::REDACTION_FILL,
        }],
    }
}

/// Conservative image-space bounds of an annotation's visuals — used for
/// viewport culling and Navigator jump targets.
pub fn annotation_bounds(annotation: &Annotation) -> ImageRect {
    match annotation {
        Annotation::NumberCallout { tip, bubble, .. } => {
            let r = style::NUMBER_BUBBLE_RADIUS + style::NUMBER_BUBBLE_OUTLINE_WIDTH;
            let bubble_box = ImageRect {
                x: bubble.x - r,
                y: bubble.y - r,
                width: r * 2.0,
                height: r * 2.0,
            };
            // Union with the tip point (covers the leader).
            let x0 = bubble_box.x.min(tip.x);
            let y0 = bubble_box.y.min(tip.y);
            let x1 = (bubble_box.x + bubble_box.width).max(tip.x);
            let y1 = (bubble_box.y + bubble_box.height).max(tip.y);
            ImageRect { x: x0, y: y0, width: x1 - x0, height: y1 - y0 }
        }
        Annotation::TextNote { position, text, .. } => text_plate_rect(*position, text),
        Annotation::OpaqueRedaction { bounds, .. } => *bounds,
    }
}
```

Uncomment the `shapes` re-exports and `pub use text::measure_block;` in `lib.rs`.

- [ ] **Step 4: Run tests**

Run: `rtk cargo test -p rollshot-image-document`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
rtk git add crates/rollshot-image-document
rtk git commit -m "feat(image-document): shared render-shape model and annotation bounds"
```

---

### Task 9: Hit-testing

**Files:**
- Modify: `crates/rollshot-image-document/src/hit.rs`
- Modify: `crates/rollshot-image-document/src/document.rs` (wrapper method)
- Modify: `crates/rollshot-image-document/src/lib.rs` (uncomment `pub use hit::...`)

- [ ] **Step 1: Write failing tests** (bottom of `hit.rs`)

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::annotation::{Annotation, AnnotationId};
    use crate::geometry::{ImagePoint, ImageRect};
    use crate::style;

    const TOL: f32 = 8.0;

    fn callout() -> Annotation {
        Annotation::NumberCallout {
            id: AnnotationId(1),
            number: 1,
            tip: ImagePoint::new(20.0, 20.0),
            bubble: ImagePoint::new(120.0, 120.0),
        }
    }

    #[test]
    fn bubble_tip_and_miss() {
        let anns = vec![callout()];
        let bubble_hit = hit_test(&anns, ImagePoint::new(120.0, 120.0), TOL).unwrap();
        assert_eq!(bubble_hit.part, HitPart::NumberBubble);
        let tip_hit = hit_test(&anns, ImagePoint::new(22.0, 20.0), TOL).unwrap();
        assert_eq!(tip_hit.part, HitPart::NumberTip);
        assert!(hit_test(&anns, ImagePoint::new(60.0, 90.0), TOL).is_none());
    }

    #[test]
    fn bubble_edge_within_tolerance_hits() {
        let anns = vec![callout()];
        let just_outside_edge =
            ImagePoint::new(120.0 + style::NUMBER_BUBBLE_RADIUS + TOL - 1.0, 120.0);
        assert!(hit_test(&anns, just_outside_edge, TOL).is_some());
        let beyond = ImagePoint::new(120.0 + style::NUMBER_BUBBLE_RADIUS + TOL + 2.0, 120.0);
        assert!(hit_test(&anns, beyond, TOL).is_none());
    }

    #[test]
    fn text_note_body_hits_via_plate() {
        let anns = vec![Annotation::TextNote {
            id: AnnotationId(2),
            position: ImagePoint::new(10.0, 10.0),
            text: "hello".to_string(),
        }];
        let hit = hit_test(&anns, ImagePoint::new(14.0, 14.0), TOL).unwrap();
        assert_eq!(hit.part, HitPart::Body);
        assert_eq!(hit.id, AnnotationId(2));
    }

    #[test]
    fn redaction_handles_beat_body_and_corners_resolve() {
        let anns = vec![Annotation::OpaqueRedaction {
            id: AnnotationId(3),
            bounds: ImageRect { x: 50.0, y: 50.0, width: 40.0, height: 30.0 },
        }];
        let corner = hit_test(&anns, ImagePoint::new(50.0, 50.0), TOL).unwrap();
        assert_eq!(corner.part, HitPart::Resize(ResizeHandle::TopLeft));
        let edge = hit_test(&anns, ImagePoint::new(70.0, 80.0), TOL).unwrap();
        assert_eq!(edge.part, HitPart::Resize(ResizeHandle::Bottom));
        let inside = hit_test(&anns, ImagePoint::new(70.0, 65.0), TOL).unwrap();
        assert_eq!(inside.part, HitPart::Body);
    }

    #[test]
    fn topmost_annotation_wins_on_overlap() {
        let anns = vec![
            Annotation::OpaqueRedaction {
                id: AnnotationId(1),
                bounds: ImageRect { x: 0.0, y: 0.0, width: 100.0, height: 100.0 },
            },
            Annotation::OpaqueRedaction {
                id: AnnotationId(2),
                bounds: ImageRect { x: 25.0, y: 25.0, width: 100.0, height: 100.0 },
            },
        ];
        let hit = hit_test(&anns, ImagePoint::new(60.0, 60.0), TOL).unwrap();
        assert_eq!(hit.id, AnnotationId(2), "later annotations draw on top");
    }
}
```

- [ ] **Step 2: Run to verify failure**

Run: `rtk cargo test -p rollshot-image-document`
Expected: FAIL.

- [ ] **Step 3: Implement `hit.rs`**

```rust
//! Image-space hit-testing. Tolerances are passed in by the editor (which
//! converts a fixed screen-space tolerance through its zoom scale). Tip
//! tolerance factor follows the mark-shot reference
//! (learn-projects/mark-shot/src/shot_window_hit_testing.cpp:648).

use crate::annotation::{Annotation, AnnotationId};
use crate::geometry::{ImagePoint, ImageRect};
use crate::shapes::text_plate_rect;
use crate::style;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResizeHandle {
    TopLeft,
    Top,
    TopRight,
    Right,
    BottomRight,
    Bottom,
    BottomLeft,
    Left,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HitPart {
    Body,
    NumberBubble,
    NumberTip,
    Resize(ResizeHandle),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Hit {
    pub id: AnnotationId,
    pub part: HitPart,
}

/// The 8 resize-handle anchor points of a redaction (also used by the editor
/// to draw handles, so hit positions and visuals agree).
pub fn redaction_handles(bounds: ImageRect) -> [(ResizeHandle, ImagePoint); 8] {
    let (x0, y0) = (bounds.x, bounds.y);
    let (x1, y1) = (bounds.x + bounds.width, bounds.y + bounds.height);
    let (cx, cy) = (x0 + bounds.width / 2.0, y0 + bounds.height / 2.0);
    [
        (ResizeHandle::TopLeft, ImagePoint::new(x0, y0)),
        (ResizeHandle::Top, ImagePoint::new(cx, y0)),
        (ResizeHandle::TopRight, ImagePoint::new(x1, y0)),
        (ResizeHandle::Right, ImagePoint::new(x1, cy)),
        (ResizeHandle::BottomRight, ImagePoint::new(x1, y1)),
        (ResizeHandle::Bottom, ImagePoint::new(cx, y1)),
        (ResizeHandle::BottomLeft, ImagePoint::new(x0, y1)),
        (ResizeHandle::Left, ImagePoint::new(x0, cy)),
    ]
}

fn hit_annotation(annotation: &Annotation, point: ImagePoint, tolerance: f32) -> Option<HitPart> {
    match annotation {
        Annotation::NumberCallout { tip, bubble, .. } => {
            if point.distance(*bubble) <= style::NUMBER_BUBBLE_RADIUS + tolerance {
                Some(HitPart::NumberBubble)
            } else if point.distance(*tip) <= tolerance * 1.6 {
                Some(HitPart::NumberTip)
            } else {
                None
            }
        }
        Annotation::TextNote { position, text, .. } => text_plate_rect(*position, text)
            .expanded(tolerance)
            .contains(point)
            .then_some(HitPart::Body),
        Annotation::OpaqueRedaction { bounds, .. } => {
            for (handle, anchor) in redaction_handles(*bounds) {
                if point.distance(anchor) <= tolerance * 1.5 {
                    return Some(HitPart::Resize(handle));
                }
            }
            bounds.expanded(tolerance).contains(point).then_some(HitPart::Body)
        }
    }
}

/// Topmost hit at `point` (later annotations paint on top, so scan reversed).
/// First release: linear scan, no spatial index (spec §13).
pub fn hit_test(annotations: &[Annotation], point: ImagePoint, tolerance: f32) -> Option<Hit> {
    annotations.iter().rev().find_map(|a| {
        hit_annotation(a, point, tolerance).map(|part| Hit { id: a.id(), part })
    })
}
```

Add the document wrapper in `document.rs` (`impl ImageDocument`):

```rust
    pub fn hit_test(&self, point: ImagePoint, tolerance: f32) -> Option<Hit> {
        crate::hit::hit_test(&self.annotations, point, tolerance)
    }
```

Uncomment `pub use hit::{redaction_handles, Hit, HitPart, ResizeHandle};` in `lib.rs`.

- [ ] **Step 4: Run tests**

Run: `rtk cargo test -p rollshot-image-document`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
rtk git add crates/rollshot-image-document
rtk git commit -m "feat(image-document): annotation hit-testing with handle parts"
```

---

### Task 10: Navigator ordering

**Files:**
- Modify: `crates/rollshot-image-document/src/navigator.rs`
- Modify: `crates/rollshot-image-document/src/document.rs` (wrapper method)
- Modify: `crates/rollshot-image-document/src/lib.rs` (uncomment `pub use navigator::NavigatorItem;`)

- [ ] **Step 1: Write failing tests** (bottom of `navigator.rs`)

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::annotation::{Annotation, AnnotationId};
    use crate::geometry::{ImagePoint, ImageRect};

    #[test]
    fn items_sort_by_y_then_x_then_id() {
        let anns = vec![
            Annotation::NumberCallout {
                id: AnnotationId(1),
                number: 1,
                tip: ImagePoint::new(0.0, 500.0),
                bubble: ImagePoint::new(10.0, 500.0),
            },
            Annotation::TextNote {
                id: AnnotationId(2),
                position: ImagePoint::new(40.0, 100.0),
                text: "note".to_string(),
            },
            // Same y as the text note, smaller x → sorts first of the two.
            Annotation::OpaqueRedaction {
                id: AnnotationId(3),
                bounds: ImageRect { x: 5.0, y: 100.0, width: 10.0, height: 10.0 },
            },
        ];
        let order: Vec<AnnotationId> = navigator_items(&anns).iter().map(|i| i.id).collect();
        assert_eq!(order, vec![AnnotationId(3), AnnotationId(2), AnnotationId(1)]);
    }

    #[test]
    fn exact_ties_fall_back_to_stable_id() {
        let at = ImagePoint::new(50.0, 50.0);
        let anns = vec![
            Annotation::TextNote { id: AnnotationId(9), position: at, text: "b".into() },
            Annotation::TextNote { id: AnnotationId(4), position: at, text: "a".into() },
        ];
        let order: Vec<AnnotationId> = navigator_items(&anns).iter().map(|i| i.id).collect();
        assert_eq!(order, vec![AnnotationId(4), AnnotationId(9)]);
    }

    #[test]
    fn labels_show_number_text_summary_and_redaction() {
        let anns = vec![
            Annotation::NumberCallout {
                id: AnnotationId(1),
                number: 7,
                tip: ImagePoint::new(0.0, 0.0),
                bubble: ImagePoint::new(0.0, 0.0),
            },
            Annotation::TextNote {
                id: AnnotationId(2),
                position: ImagePoint::new(0.0, 10.0),
                text: "first line is quite long and gets truncated\nsecond".to_string(),
            },
            Annotation::OpaqueRedaction {
                id: AnnotationId(3),
                bounds: ImageRect { x: 0.0, y: 20.0, width: 5.0, height: 5.0 },
            },
        ];
        let items = navigator_items(&anns);
        assert_eq!(items[0].label, "7");
        assert_eq!(items[1].label, "first line is quite long…");
        assert!(items[1].label.chars().count() <= 25);
        assert_eq!(items[2].label, "Redaction");
    }
}
```

- [ ] **Step 2: Run to verify failure**

Run: `rtk cargo test -p rollshot-image-document`
Expected: FAIL.

- [ ] **Step 3: Implement `navigator.rs`**

```rust
//! Deterministic Navigator ordering (spec §8.2): image-space top-to-bottom,
//! ties by horizontal position, then stable annotation ID.

use crate::annotation::{Annotation, AnnotationId};
use crate::geometry::ImagePoint;
use crate::shapes::annotation_bounds;

const TEXT_SUMMARY_CHARS: usize = 24;

#[derive(Debug, Clone, PartialEq)]
pub struct NavigatorItem {
    pub id: AnnotationId,
    pub label: String,
    /// Visual center, the Navigator jump target (spec §8.2).
    pub center: ImagePoint,
}

fn label(annotation: &Annotation) -> String {
    match annotation {
        Annotation::NumberCallout { number, .. } => number.to_string(),
        Annotation::TextNote { text, .. } => {
            let first_line = text.lines().next().unwrap_or("").trim();
            let mut summary: String = first_line.chars().take(TEXT_SUMMARY_CHARS).collect();
            if first_line.chars().count() > TEXT_SUMMARY_CHARS {
                summary.push('…');
            }
            summary
        }
        Annotation::OpaqueRedaction { .. } => "Redaction".to_string(),
    }
}

pub fn navigator_items(annotations: &[Annotation]) -> Vec<NavigatorItem> {
    let mut items: Vec<(ImagePoint, NavigatorItem)> = annotations
        .iter()
        .map(|a| {
            let anchor = a.anchor();
            (
                anchor,
                NavigatorItem {
                    id: a.id(),
                    label: label(a),
                    center: annotation_bounds(a).center(),
                },
            )
        })
        .collect();
    items.sort_by(|(a, ia), (b, ib)| {
        a.y.total_cmp(&b.y)
            .then(a.x.total_cmp(&b.x))
            .then(ia.id.cmp(&ib.id))
    });
    items.into_iter().map(|(_, item)| item).collect()
}
```

Add the document wrapper in `document.rs` (`impl ImageDocument`):

```rust
    pub fn navigator_items(&self) -> Vec<NavigatorItem> {
        crate::navigator::navigator_items(&self.annotations)
    }
```

Uncomment the navigator re-export in `lib.rs`.

- [ ] **Step 4: Run tests**

Run: `rtk cargo test -p rollshot-image-document`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
rtk git add crates/rollshot-image-document
rtk git commit -m "feat(image-document): deterministic navigator ordering and labels"
```

---

### Task 11: Rasterizer and flatten

**Files:**
- Modify: `crates/rollshot-image-document/src/raster.rs` (extend; `blend_px` exists from Task 4)
- Modify: `crates/rollshot-image-document/src/flatten.rs`
- Modify: `crates/rollshot-image-document/src/document.rs` (`flatten` method)

- [ ] **Step 1: Write failing tests** (bottom of `flatten.rs`)

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::annotation::{Annotation, AnnotationId};
    use crate::geometry::{ImagePoint, ImageRect};
    use crate::ImageDocument;
    use image::{Rgba, RgbaImage};

    fn base(w: u32, h: u32) -> RgbaImage {
        RgbaImage::from_pixel(w, h, Rgba([10, 20, 30, 255]))
    }

    #[test]
    fn flatten_with_no_annotations_equals_source_and_source_is_untouched() {
        let doc = ImageDocument::new(base(50, 50));
        let out = doc.flatten();
        assert_eq!(out.as_raw(), doc.source().as_raw());
    }

    #[test]
    fn redaction_replaces_covered_pixels_exactly_opaque() {
        let mut doc = ImageDocument::new(base(50, 50));
        doc.add_redaction(ImageRect { x: 10.0, y: 10.0, width: 20.0, height: 20.0 })
            .unwrap();
        let out = doc.flatten();
        // Interior pixel: exactly the fill, fully opaque — no blending, no
        // recoverable trace of the source (spec §9.4).
        assert_eq!(out.get_pixel(20, 20).0, [0, 0, 0, 255]);
        // Outside pixel untouched.
        assert_eq!(out.get_pixel(5, 5).0, [10, 20, 30, 255]);
        // Source untouched.
        assert_eq!(doc.source().get_pixel(20, 20).0, [10, 20, 30, 255]);
    }

    #[test]
    fn number_callout_paints_accent_bubble_and_white_label() {
        let mut doc = ImageDocument::new(base(200, 200));
        doc.add_number_callout(ImagePoint::new(30.0, 30.0), ImagePoint::new(100.0, 100.0));
        let out = doc.flatten();
        // Bubble center area is accent-colored.
        let center = out.get_pixel(100, 100).0;
        // Center pixel may be white (label stroke) or accent; check a ring
        // pixel inside the bubble but off the glyph.
        let ring = out.get_pixel(110, 100).0;
        assert!(
            center != [10, 20, 30, 255] && ring != [10, 20, 30, 255],
            "bubble must paint over the source"
        );
        // Some white label pixels exist near the center.
        let mut white_nearby = 0;
        for y in 90..110 {
            for x in 90..110 {
                let p = out.get_pixel(x, y).0;
                if p[0] > 200 && p[1] > 200 && p[2] > 200 {
                    white_nearby += 1;
                }
            }
        }
        assert!(white_nearby > 3, "expected white label pixels, got {white_nearby}");
        // Leader direction: a pixel on the line toward the tip is painted.
        let leader = out.get_pixel(80, 80).0;
        assert_ne!(leader, [10, 20, 30, 255], "leader triangle paints toward the tip");
    }

    #[test]
    fn text_note_paints_plate_and_glyphs() {
        let mut doc = ImageDocument::new(base(300, 100));
        doc.add_text_note(ImagePoint::new(10.0, 10.0), "Hello".to_string()).unwrap();
        let out = doc.flatten();
        let plate = out.get_pixel(14, 14).0;
        assert!(plate[0] < 30 && plate[1] < 30 && plate[2] < 30, "dark plate expected");
        let changed = out
            .pixels()
            .zip(doc.source().pixels())
            .filter(|(a, b)| a != b)
            .count();
        assert!(changed > 100, "plate + glyphs change many pixels, got {changed}");
    }

    #[test]
    fn flatten_excludes_nothing_committed_and_is_repeatable() {
        let mut doc = ImageDocument::new(base(100, 100));
        doc.add_number_callout(ImagePoint::new(10.0, 10.0), ImagePoint::new(10.0, 10.0));
        let first = doc.flatten();
        let second = doc.flatten();
        assert_eq!(first.as_raw(), second.as_raw(), "flatten is deterministic");
    }

    /// Spec §13/§16: long image at the history-limit annotation scale.
    #[test]
    fn hundred_annotations_on_long_image_flatten_hit_test_and_order() {
        let mut doc = ImageDocument::new(base(1000, 20000));
        for i in 0..34u32 {
            let y = 100.0 + i as f32 * 580.0;
            doc.add_number_callout(ImagePoint::new(100.0, y), ImagePoint::new(160.0, y));
            doc.add_text_note(ImagePoint::new(300.0, y), format!("step {i}")).unwrap();
            doc.add_redaction(ImageRect { x: 500.0, y, width: 80.0, height: 40.0 }).unwrap();
        }
        assert_eq!(doc.annotations().len(), 102);
        let items = doc.navigator_items();
        assert_eq!(items.len(), 102);
        assert!(doc.hit_test(ImagePoint::new(160.0, 100.0), 8.0).is_some());
        let out = doc.flatten();
        assert_eq!(out.dimensions(), (1000, 20000));
    }
}
```

- [ ] **Step 2: Run to verify failure**

Run: `rtk cargo test -p rollshot-image-document`
Expected: FAIL — `flatten` not defined.

- [ ] **Step 3: Extend `raster.rs`** (below `blend_px`)

```rust
use crate::geometry::{ImagePoint, ImageRect};

/// Solid rectangle fill. Edges snap to whole pixels (crisp redactions); the
/// blend at alpha 255 replaces pixels exactly.
pub(crate) fn fill_rect(img: &mut RgbaImage, rect: ImageRect, color: Rgba8) {
    let x0 = rect.x.round() as i32;
    let y0 = rect.y.round() as i32;
    let x1 = (rect.x + rect.width).round() as i32;
    let y1 = (rect.y + rect.height).round() as i32;
    for y in y0..y1 {
        for x in x0..x1 {
            blend_px(img, x, y, color, 1.0);
        }
    }
}

/// Anti-aliased filled circle: per-pixel coverage from distance to center.
pub(crate) fn fill_circle(img: &mut RgbaImage, center: ImagePoint, radius: f32, color: Rgba8) {
    let x0 = (center.x - radius - 1.0).floor() as i32;
    let y0 = (center.y - radius - 1.0).floor() as i32;
    let x1 = (center.x + radius + 1.0).ceil() as i32;
    let y1 = (center.y + radius + 1.0).ceil() as i32;
    for y in y0..=y1 {
        for x in x0..=x1 {
            let d = ImagePoint::new(x as f32 + 0.5, y as f32 + 0.5).distance(center);
            let coverage = (radius + 0.5 - d).clamp(0.0, 1.0);
            blend_px(img, x, y, color, coverage);
        }
    }
}

/// Anti-aliased ring (circle outline) of `width` centered on `radius`.
pub(crate) fn stroke_circle(
    img: &mut RgbaImage,
    center: ImagePoint,
    radius: f32,
    width: f32,
    color: Rgba8,
) {
    let outer = radius + width / 2.0;
    let inner = radius - width / 2.0;
    let x0 = (center.x - outer - 1.0).floor() as i32;
    let y0 = (center.y - outer - 1.0).floor() as i32;
    let x1 = (center.x + outer + 1.0).ceil() as i32;
    let y1 = (center.y + outer + 1.0).ceil() as i32;
    for y in y0..=y1 {
        for x in x0..=x1 {
            let d = ImagePoint::new(x as f32 + 0.5, y as f32 + 0.5).distance(center);
            let coverage =
                ((outer + 0.5 - d).clamp(0.0, 1.0)) * ((d - inner + 0.5).clamp(0.0, 1.0));
            blend_px(img, x, y, color, coverage);
        }
    }
}

fn edge(a: ImagePoint, b: ImagePoint, p: ImagePoint) -> f32 {
    (b.x - a.x) * (p.y - a.y) - (b.y - a.y) * (p.x - a.x)
}

fn point_in_triangle(p: ImagePoint, t: &[ImagePoint; 3]) -> bool {
    let d1 = edge(t[0], t[1], p);
    let d2 = edge(t[1], t[2], p);
    let d3 = edge(t[2], t[0], p);
    let has_neg = d1 < 0.0 || d2 < 0.0 || d3 < 0.0;
    let has_pos = d1 > 0.0 || d2 > 0.0 || d3 > 0.0;
    !(has_neg && has_pos)
}

/// Anti-aliased filled triangle via 4×4 supersampled coverage.
pub(crate) fn fill_triangle(img: &mut RgbaImage, t: &[ImagePoint; 3], color: Rgba8) {
    let xs = [t[0].x, t[1].x, t[2].x];
    let ys = [t[0].y, t[1].y, t[2].y];
    let x0 = xs.iter().cloned().fold(f32::MAX, f32::min).floor() as i32;
    let y0 = ys.iter().cloned().fold(f32::MAX, f32::min).floor() as i32;
    let x1 = xs.iter().cloned().fold(f32::MIN, f32::max).ceil() as i32;
    let y1 = ys.iter().cloned().fold(f32::MIN, f32::max).ceil() as i32;
    for y in y0..=y1 {
        for x in x0..=x1 {
            let mut hits = 0u32;
            for sy in 0..4 {
                for sx in 0..4 {
                    let sample = ImagePoint::new(
                        x as f32 + (sx as f32 + 0.5) / 4.0,
                        y as f32 + (sy as f32 + 0.5) / 4.0,
                    );
                    if point_in_triangle(sample, t) {
                        hits += 1;
                    }
                }
            }
            if hits > 0 {
                blend_px(img, x, y, color, hits as f32 / 16.0);
            }
        }
    }
}
```

- [ ] **Step 4: Implement `flatten.rs`**

```rust
//! Flatten committed annotations onto a copy of the full-resolution source
//! (spec §11.2). Selection, hover, viewport, and drafts never reach this
//! module — it consumes only the committed annotation graph.

use image::RgbaImage;

use crate::annotation::Annotation;
use crate::geometry::ImagePoint;
use crate::raster::{fill_circle, fill_rect, fill_triangle, stroke_circle};
use crate::shapes::{annotation_shapes, RenderShape, TextAnchor};
use crate::text::{draw_block, measure_block};

pub(crate) fn flatten_onto(source: &RgbaImage, annotations: &[Annotation]) -> RgbaImage {
    let mut out = source.clone();
    for annotation in annotations {
        for shape in annotation_shapes(annotation) {
            draw_shape(&mut out, &shape);
        }
    }
    out
}

fn draw_shape(img: &mut RgbaImage, shape: &RenderShape) {
    match shape {
        RenderShape::Rect { rect, color } => fill_rect(img, *rect, *color),
        RenderShape::Circle { center, radius, fill, outline_width, outline } => {
            fill_circle(img, *center, *radius, *fill);
            if *outline_width > 0.0 {
                stroke_circle(img, *center, *radius, *outline_width, *outline);
            }
        }
        RenderShape::Triangle { points, color } => fill_triangle(img, points, *color),
        RenderShape::Label { anchor, anchor_kind, content, px, bold, color } => {
            let top_left = match anchor_kind {
                TextAnchor::TopLeft => *anchor,
                TextAnchor::Center => {
                    let (w, h) = measure_block(content, *px, *bold);
                    ImagePoint::new(anchor.x - w / 2.0, anchor.y - h / 2.0)
                }
            };
            draw_block(img, top_left, content, *px, *bold, *color);
        }
    }
}
```

Add the document method in `document.rs` (`impl ImageDocument`):

```rust
    /// Render the annotated full-resolution output. Infallible and
    /// non-mutating; called only for explicit Copy/Save actions (spec §11.2).
    pub fn flatten(&self) -> RgbaImage {
        crate::flatten::flatten_onto(&self.source, &self.annotations)
    }
```

- [ ] **Step 5: Run all crate tests**

Run: `rtk cargo test -p rollshot-image-document`
Expected: PASS (all tasks' tests).

- [ ] **Step 6: Format and lint**

Run: `rtk cargo fmt --check` (fix with `rtk cargo fmt` if needed)
Run: `rtk cargo clippy -p rollshot-image-document --all-targets -- -D warnings`
Expected: clean.

- [ ] **Step 7: Commit**

```bash
rtk git add crates/rollshot-image-document
rtk git commit -m "feat(image-document): AA rasterizer and full-resolution flatten"
```

---

# Phase 2 — `rollshot-app` Result Workspace integration

### Task 12: App wiring and the long-image rule

**Files:**
- Modify: `crates/rollshot-app/Cargo.toml`
- Modify: `crates/rollshot-app/src/result_workspace/viewport.rs`

- [ ] **Step 1: Add dependency and iced features**

In `crates/rollshot-app/Cargo.toml`, change the iced line and add the document crate:

```toml
iced = { version = "0.14", features = ["canvas", "image", "tokio"] }
rollshot-image-document = { path = "../rollshot-image-document" }
```

(`canvas`/`image` were previously only enabled transitively through `rollshot-iced-overlay`; the app now uses them directly.)

- [ ] **Step 2: Write failing test** (in `viewport.rs` `mod tests`)

```rust
    #[test]
    fn tall_image_rule_matches_default_zoom_fit_width_arm() {
        assert!(is_tall_image(Size::new(800.0, 2401.0)));
        assert!(!is_tall_image(Size::new(500.0, 1000.0)), "exactly 2× is not tall");
        assert!(!is_tall_image(Size::new(1200.0, 800.0)));
    }
```

- [ ] **Step 3: Run to verify failure**

Run: `rtk cargo test -p rollshot-app tall_image`
Expected: FAIL — `is_tall_image` not defined.

- [ ] **Step 4: Implement in `viewport.rs`**

Add above `default_zoom` and refactor `default_zoom`'s first arm to use it (spec §8.2: the Navigator long-image threshold reuses this existing viewport concept):

```rust
/// "Long image" rule shared by the default-zoom choice and the Navigator's
/// default-open state (spec §8.2): strictly taller than 2× the width.
pub fn is_tall_image(image: Size) -> bool {
    image.height > 2.0 * image.width
}
```

In `default_zoom`, replace `if h > 2.0 * w {` with `if is_tall_image(image) {` (keep the horizontal arm as-is).

- [ ] **Step 5: Run tests**

Run: `rtk cargo test -p rollshot-app`
Expected: PASS — including all pre-existing viewport tests (behavior unchanged).

- [ ] **Step 6: Commit**

```bash
rtk git add crates/rollshot-app
rtk git commit -m "feat(app): wire image-document dependency and extract tall-image rule"
```

---

### Task 13: Mechanical module split of result_workspace

Pure code motion — NO behavior change (spec §14). The public-to-crate API must stay importable at the same paths because `macos_product.rs` uses them.

**Files:**
- Modify: `crates/rollshot-app/src/result_workspace/mod.rs`
- Create: `crates/rollshot-app/src/result_workspace/document.rs`
- Create: `crates/rollshot-app/src/result_workspace/update.rs`
- Create: `crates/rollshot-app/src/result_workspace/view.rs`

- [ ] **Step 1: Record the baseline**

Run: `rtk cargo test -p rollshot-app 2>&1 | tail -3` and note the passing count.

- [ ] **Step 2: Move code**

Distribute `mod.rs` content (move, do not rewrite):

- → `document.rs`: `ResultDocument`, `CloseDecision`, `close_decision`, `default_save_name`, `UNSAVED_LABEL`, `DISCARD_PROMPT`, and their tests (`saved_document_closes_immediately`, `unsaved_document_requests_discard_confirmation`).
- → `update.rs`: `Message`, `update`, `zoom_modifier_held`, `apply_zoom_at_pointer`, `handle_wheel`, `scroll_delta_pixels`, `subscription`, and all update-driven tests.
- → `view.rs`: `view`, `reveal_button`, `message_row`, `canvas_view`, `thick_scrollbar`, `status_bar`, `zoom_label`, `discard_modal`, plus the layout constants (`SCROLLBAR_WIDTH`, `SCROLLBAR_SPACING`).
- stays in `mod.rs`: module decls, `ResultWorkspace`, `InlineMessage`, `build_display_handle`, `run`, `SUCCESS_MESSAGE_DURATION`, `WHEEL_LINE_PX`, the construction/`apply_save_as` tests.

Top of new `mod.rs`:

```rust
pub mod actions;
mod document;
mod update;
mod view;
pub mod viewport;

pub use document::{close_decision, CloseDecision, ResultDocument};
pub use update::Message;
pub(crate) use update::{subscription, update};
pub(crate) use view::view;
```

Use `pub(crate)`/`pub(super)` visibility within the submodules as needed for cross-module access (e.g. `view.rs` reads `ResultWorkspace` fields; `update.rs` calls `actions::*`).

- [ ] **Step 3: Verify zero behavior change**

Run: `rtk cargo test -p rollshot-app`
Expected: PASS with the same test count as Step 1.
Run: `rtk cargo check -p rollshot-app`
Expected: clean (on Linux this does not compile `macos_product.rs`; its imports go through the re-exports above, which is why they must exist — flag macOS compile verification in the final task).

- [ ] **Step 4: Commit**

```bash
rtk git add crates/rollshot-app
rtk git commit -m "refactor(app): split result_workspace into document/update/view modules"
```

---

### Task 14: ResultDocument on ImageDocument — paths, dirty state, close rules

**Files:**
- Modify: `crates/rollshot-app/src/result_workspace/document.rs`
- Modify: `crates/rollshot-app/src/result_workspace/mod.rs`
- Modify: `crates/rollshot-app/src/result_workspace/update.rs` (compile fixes)
- Modify: `crates/rollshot-app/src/result_workspace/view.rs` (compile fixes)

First locate every touchpoint: `rtk grep -rn "source_image\|saved_path\|confirming_discard" crates/rollshot-app/src` — expect hits in `result_workspace/*` and `macos_product.rs` (constructor calls only: `ResultDocument::unsaved/saved`, which keep their signatures).

- [ ] **Step 1: Write failing tests** (replace/extend in `document.rs` tests; add workspace-level tests in `mod.rs`)

```rust
    // document.rs tests
    use image::{Rgba, RgbaImage};
    use rollshot_image_document::ImagePoint;

    fn image() -> RgbaImage {
        RgbaImage::from_pixel(2, 2, Rgba([1, 2, 3, 255]))
    }

    #[test]
    fn clean_saved_document_closes_immediately() {
        let d = ResultDocument::saved(image(), PathBuf::from("/tmp/a.png"));
        assert_eq!(close_decision(&d, false), CloseDecision::Close);
    }

    #[test]
    fn unsaved_capture_without_export_confirms_capture_loss() {
        let d = ResultDocument::unsaved(image());
        assert_eq!(
            close_decision(&d, false),
            CloseDecision::Confirm(DiscardPrompt { lose_capture: true, lose_edits: false })
        );
    }

    #[test]
    fn dirty_annotations_confirm_edit_loss_even_when_saved() {
        let d = ResultDocument::saved(image(), PathBuf::from("/tmp/a.png"));
        assert_eq!(
            close_decision(&d, true),
            CloseDecision::Confirm(DiscardPrompt { lose_capture: false, lose_edits: true })
        );
    }

    #[test]
    fn unsaved_capture_with_successful_export_closes_when_clean() {
        let mut d = ResultDocument::unsaved(image());
        d.last_export_path = Some(PathBuf::from("/tmp/out.png"));
        assert_eq!(close_decision(&d, false), CloseDecision::Close,
            "spec §12.3: a successful Save As permits closing");
    }

    #[test]
    fn prompt_text_distinguishes_capture_edits_and_both() {
        assert_eq!(
            DiscardPrompt { lose_capture: true, lose_edits: false }.text(),
            "Discard unsaved capture?"
        );
        assert_eq!(
            DiscardPrompt { lose_capture: false, lose_edits: true }.text(),
            "Discard annotation edits?"
        );
        assert_eq!(
            DiscardPrompt { lose_capture: true, lose_edits: true }.text(),
            "Discard unsaved capture and annotation edits?"
        );
    }

    #[test]
    fn reveal_path_prefers_export_then_source() {
        let mut d = ResultDocument::saved(image(), PathBuf::from("/tmp/src.png"));
        assert_eq!(d.reveal_path(), Some(Path::new("/tmp/src.png")));
        d.last_export_path = Some(PathBuf::from("/tmp/out.png"));
        assert_eq!(d.reveal_path(), Some(Path::new("/tmp/out.png")));
        assert!(ResultDocument::unsaved(image()).reveal_path().is_none());
    }
```

```rust
    // mod.rs workspace tests (replacing the old saved_path-based ones)
    #[test]
    fn save_as_updates_export_path_not_source_path_and_clears_dirty() {
        let mut state = unsaved_workspace();
        state
            .document
            .image
            .add_number_callout(
                rollshot_image_document::ImagePoint::new(1.0, 1.0),
                rollshot_image_document::ImagePoint::new(1.0, 1.0),
            );
        assert!(state.annotations_dirty());
        state.apply_save_as(Ok(Some(PathBuf::from("/tmp/out.png"))));
        assert_eq!(state.document.source_path, None, "source identity preserved");
        assert_eq!(
            state.document.last_export_path.as_deref(),
            Some(Path::new("/tmp/out.png"))
        );
        assert!(!state.annotations_dirty(), "successful Save As clears dirty");
    }

    #[test]
    fn editing_after_save_as_makes_dirty_again_and_undo_to_saved_is_clean() {
        let mut state = unsaved_workspace();
        state.apply_save_as(Ok(Some(PathBuf::from("/tmp/out.png"))));
        state.document.image.add_number_callout(
            rollshot_image_document::ImagePoint::new(1.0, 1.0),
            rollshot_image_document::ImagePoint::new(1.0, 1.0),
        );
        assert!(state.annotations_dirty());
        assert!(state.document.image.undo());
        assert!(!state.annotations_dirty(), "undo back to the saved state is clean");
    }

    #[test]
    fn failed_save_as_leaves_paths_and_dirty_state_unchanged() {
        let mut state = unsaved_workspace();
        state.document.image.add_number_callout(
            rollshot_image_document::ImagePoint::new(1.0, 1.0),
            rollshot_image_document::ImagePoint::new(1.0, 1.0),
        );
        state.apply_save_as(Err("disk full".to_string()));
        assert!(state.document.last_export_path.is_none());
        assert!(state.annotations_dirty());
        assert!(matches!(&state.message, Some(InlineMessage::Error(_))));
    }

    #[test]
    fn navigator_defaults_open_for_tall_images_only() {
        let short = ResultWorkspace::new(ResultDocument::unsaved(image()), None);
        assert!(!short.editor.navigator_open);
        let tall_img = RgbaImage::from_pixel(100, 300, Rgba([0, 0, 0, 255]));
        let tall = ResultWorkspace::new(ResultDocument::unsaved(tall_img), None);
        assert!(tall.editor.navigator_open);
    }
```

- [ ] **Step 2: Run to verify failure**

Run: `rtk cargo test -p rollshot-app`
Expected: FAIL / compile errors.

- [ ] **Step 3: Rewrite `document.rs` core**

```rust
use std::path::{Path, PathBuf};

use image::RgbaImage;
use rollshot_image_document::ImageDocument;

pub(crate) const UNSAVED_LABEL: &str = "Unsaved capture";

/// The Result Workspace document: the image document plus durable-path
/// identity (spec §7). `source_path` is the original auto-saved capture and
/// never changes because of annotation export; `last_export_path` is the most
/// recent successful annotated Save As.
pub struct ResultDocument {
    pub image: ImageDocument,
    pub source_path: Option<PathBuf>,
    pub last_export_path: Option<PathBuf>,
}

impl ResultDocument {
    pub fn saved(image: RgbaImage, path: PathBuf) -> Self {
        Self {
            image: ImageDocument::new(image),
            source_path: Some(path),
            last_export_path: None,
        }
    }

    pub fn unsaved(image: RgbaImage) -> Self {
        Self {
            image: ImageDocument::new(image),
            source_path: None,
            last_export_path: None,
        }
    }

    /// Reveal opens the latest durable output, preferring the annotated
    /// export over the original (spec §7).
    pub fn reveal_path(&self) -> Option<&Path> {
        self.last_export_path.as_deref().or(self.source_path.as_deref())
    }

    pub fn display_name(&self) -> String {
        self.source_path
            .as_deref()
            .or(self.last_export_path.as_deref())
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| UNSAVED_LABEL.to_string())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DiscardPrompt {
    pub lose_capture: bool,
    pub lose_edits: bool,
}

impl DiscardPrompt {
    pub fn text(&self) -> &'static str {
        match (self.lose_capture, self.lose_edits) {
            (true, true) => "Discard unsaved capture and annotation edits?",
            (true, false) => "Discard unsaved capture?",
            _ => "Discard annotation edits?",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloseDecision {
    Close,
    Confirm(DiscardPrompt),
}

/// Spec §12.3: confirm when the capture has no durable file at all, or when
/// annotation edits are dirty relative to the last successful Save As.
pub fn close_decision(document: &ResultDocument, annotations_dirty: bool) -> CloseDecision {
    let lose_capture = document.source_path.is_none() && document.last_export_path.is_none();
    if lose_capture || annotations_dirty {
        CloseDecision::Confirm(DiscardPrompt { lose_capture, lose_edits: annotations_dirty })
    } else {
        CloseDecision::Close
    }
}
```

Keep `default_save_name` but read `document.source_path` instead of `saved_path`.

- [ ] **Step 4: Update `ResultWorkspace` in `mod.rs`**

- Replace field `confirming_discard: bool` with `pending_discard: Option<document::DiscardPrompt>` (init `None`).
- Add fields:

```rust
    /// UI/session editor state (active tool, selection, drafts, Navigator).
    pub editor: canvas::EditorState,
    /// Identity of the inline text editor widget, for focus operations.
    pub text_editor_id: iced::widget::Id,
```

Create `canvas.rs` now as a stub (Task 16 replaces it) and declare it in `mod.rs` (`pub(crate) mod canvas;`):

```rust
//! Editor/session state. Stub — expanded in the editor-state task.
pub struct EditorState {
    pub navigator_open: bool,
    pub saved_state_id: u64,
}

impl EditorState {
    pub fn new(saved_state_id: u64, navigator_open: bool) -> Self {
        Self { navigator_open, saved_state_id }
    }
}
```

(The `EditorState::new(saved_state_id, navigator_open)` signature is kept verbatim by the Task 16 replacement.) The `use rollshot_image_document::ImagePoint;` line in the Step 1 `document.rs` test block is only needed if you add geometry assertions there — drop it if the compiler flags it unused.

- In `ResultWorkspace::with_max_texture_dim`: build sizes from `document.image.source()`, set

```rust
            editor: canvas::EditorState::new(
                document.image.state_id(),
                viewport::is_tall_image(source_size),
            ),
            text_editor_id: iced::widget::Id::unique(),
            pending_discard: None,
```

- Add the dirty accessor:

```rust
    pub fn annotations_dirty(&self) -> bool {
        self.document.image.state_id() != self.editor.saved_state_id
    }
```

- Rewrite `apply_save_as`:

```rust
    pub fn apply_save_as(&mut self, result: Result<Option<PathBuf>, String>) {
        match result {
            Ok(Some(path)) => {
                let text = format!("Saved to {}", path.display());
                self.document.last_export_path = Some(path);
                self.editor.saved_state_id = self.document.image.state_id();
                self.message = Some(InlineMessage::success(text));
                self.pending_discard = None;
            }
            Ok(None) => {}
            Err(e) => self.message = Some(InlineMessage::Error(e)),
        }
    }
```

- `can_reveal`: `self.document.reveal_path().is_some()`.
- `original_size`: from `self.document.image.source()`.
- Initial message: `document.source_path` instead of `saved_path`.

- [ ] **Step 5: Fix `update.rs` and `view.rs` references**

- `Message::RequestClose` arm: `match close_decision(&state.document, state.annotations_dirty())` — `Confirm(prompt)` sets `state.pending_discard = Some(prompt)`.
- `Message::KeepUnsaved`: `state.pending_discard = None;`.
- `Message::Copy` arm: leave routing as-is for now (Task 15 changes it); fix the field access to `state.document.image.source()`.
- `Message::SaveAs`/`SavePathChosen`: same field fix.
- `Message::Reveal`: `let Some(path) = state.document.reveal_path().map(Path::to_path_buf) else { return Task::none(); };`.
- `view.rs`: title via `state.document.display_name()`; discard modal renders `prompt.text()` from `state.pending_discard` (`if let Some(prompt) = &state.pending_discard`).
- Update the existing tests that referenced `confirming_discard`/`saved_path` to the new fields (`pending_discard.is_some()`, `document.source_path`, `document.last_export_path`).

- [ ] **Step 6: Run tests**

Run: `rtk cargo test -p rollshot-app`
Expected: PASS — all new and adapted tests.

- [ ] **Step 7: Commit**

```bash
rtk git add crates/rollshot-app
rtk git commit -m "feat(app): result document on image-document with export path and dirty rules"
```

---

### Task 15: Copy ▾ / Save As / Reveal routing on the flattened document, app fonts

**Files:**
- Modify: `crates/rollshot-app/src/result_workspace/update.rs`
- Modify: `crates/rollshot-app/src/result_workspace/mod.rs` (`run` font loading)
- Modify: `crates/rollshot-app/src/macos_product.rs` (daemon font loading only)

- [ ] **Step 1: Write failing tests** (in `update.rs` tests)

```rust
    use rollshot_image_document::{ImagePoint, ImageRect};

    #[test]
    fn copy_flattens_annotations_and_does_not_clear_dirty() {
        let mut state = unsaved_workspace();
        state
            .document
            .image
            .add_redaction(ImageRect { x: 0.0, y: 0.0, width: 1.0, height: 1.0 })
            .unwrap();
        assert!(state.annotations_dirty());
        // Routing test: the Copy arm must produce the flattened image. We test
        // the pure helper rather than the clipboard side effect.
        let flattened = copy_payload(&state);
        assert_ne!(
            flattened.get_pixel(0, 0).0,
            state.document.image.source().get_pixel(0, 0).0,
            "copy payload is the flattened image"
        );
        assert!(state.annotations_dirty(), "spec §12.1: copy never clears dirty");
    }

    #[test]
    fn copy_original_payload_is_the_source() {
        let mut state = unsaved_workspace();
        state
            .document
            .image
            .add_redaction(ImageRect { x: 0.0, y: 0.0, width: 1.0, height: 1.0 })
            .unwrap();
        let original = copy_original_payload(&state);
        assert_eq!(original.as_raw(), state.document.image.source().as_raw());
    }

    #[test]
    fn save_payload_is_source_without_annotations_and_flatten_with() {
        let mut state = unsaved_workspace();
        assert_eq!(
            save_payload(&state).as_raw(),
            state.document.image.source().as_raw(),
            "spec §12.2: no annotations → original bytes"
        );
        state.document.image.add_number_callout(
            ImagePoint::new(1.0, 1.0),
            ImagePoint::new(1.0, 1.0),
        );
        assert_ne!(save_payload(&state).as_raw(), state.document.image.source().as_raw());
    }
```

- [ ] **Step 2: Run to verify failure**

Run: `rtk cargo test -p rollshot-app copy_`
Expected: FAIL — helpers not defined.

- [ ] **Step 3: Implement payload helpers and rewire arms** (`update.rs`)

```rust
/// The image Copy places on the clipboard: always the flattened document
/// (pixel-identical to the source when no annotations exist — spec §12.1).
pub(crate) fn copy_payload(state: &ResultWorkspace) -> image::RgbaImage {
    state.document.image.flatten()
}

pub(crate) fn copy_original_payload(state: &ResultWorkspace) -> image::RgbaImage {
    state.document.image.source().clone()
}

/// The image Save As writes: original bytes when no annotations exist,
/// otherwise the flattened document (spec §12.2).
pub(crate) fn save_payload(state: &ResultWorkspace) -> image::RgbaImage {
    if state.document.image.annotations().is_empty() {
        state.document.image.source().clone()
    } else {
        state.document.image.flatten()
    }
}
```

Rewire the arms (add `Message::CopyOriginal` to the enum):

```rust
        Message::Copy => {
            let result = actions::copy_image(&copy_payload(state));
            Task::done(Message::CopyFinished(result))
        }
        Message::CopyOriginal => {
            let result = actions::copy_image(&copy_original_payload(state));
            Task::done(Message::CopyFinished(result))
        }
        Message::SavePathChosen(Some(path)) => {
            let image = save_payload(state);
            Task::perform(
                async move { actions::write_save_as(&image, &path) },
                Message::SaveFinished,
            )
        }
```

Set distinct success texts in `CopyFinished` by tracking nothing extra: change `Message::CopyFinished(Result<(), String>)` arms to message text "Copied image" (unchanged) — the annotated/original distinction lives in the action labels, not the toast.

- [ ] **Step 4: Load annotation fonts into both iced runtimes**

In `mod.rs` `run()` (Linux), chain on the application builder:

```rust
    iced::application(boot, update, view)
        .title("Rollshot")
        .font(rollshot_image_document::style::FONT_REGULAR_BYTES)
        .font(rollshot_image_document::style::FONT_BOLD_BYTES)
```

In `macos_product.rs` `iced::daemon(...)` chain (around line 558), add the same two `.font(...)` calls after `.subscription(subscription)`. This is the only macos_product change in the whole plan.

- [ ] **Step 5: Run tests**

Run: `rtk cargo test -p rollshot-app`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
rtk git add crates/rollshot-app
rtk git commit -m "feat(app): annotated copy/save payload routing and annotation fonts"
```

---

### Task 16: Editor state, tools, and editing messages (undo/redo/delete/escape)

**Files:**
- Modify: `crates/rollshot-app/src/result_workspace/canvas.rs` (replace the Task 14 stub entirely)
- Modify: `crates/rollshot-app/src/result_workspace/update.rs`

- [ ] **Step 1: Write failing tests** (in `update.rs` tests)

```rust
    use super::super::canvas::{DragState, EditorState, Tool};

    #[test]
    fn select_is_the_default_tool_and_tools_switch() {
        let mut state = unsaved_workspace();
        assert_eq!(state.editor.tool, Tool::Select);
        let _ = update(&mut state, Message::SelectTool(Tool::Number));
        assert_eq!(state.editor.tool, Tool::Number);
    }

    #[test]
    fn switching_tools_preserves_viewport() {
        let mut state = unsaved_workspace();
        state.viewport.zoom = ZoomMode::Custom(150);
        state.viewport.scroll_offset = Vector::new(11.0, 22.0);
        let _ = update(&mut state, Message::SelectTool(Tool::Redact));
        assert_eq!(state.viewport.zoom, ZoomMode::Custom(150), "spec §13");
        assert_eq!(state.viewport.scroll_offset, Vector::new(11.0, 22.0));
    }

    #[test]
    fn undo_redo_messages_drive_the_document() {
        let mut state = unsaved_workspace();
        state.document.image.add_number_callout(
            ImagePoint::new(1.0, 1.0),
            ImagePoint::new(1.0, 1.0),
        );
        let _ = update(&mut state, Message::Undo);
        assert!(state.document.image.annotations().is_empty());
        let _ = update(&mut state, Message::Redo);
        assert_eq!(state.document.image.annotations().len(), 1);
    }

    #[test]
    fn delete_removes_the_selected_annotation_and_clears_selection() {
        let mut state = unsaved_workspace();
        let id = state.document.image.add_number_callout(
            ImagePoint::new(1.0, 1.0),
            ImagePoint::new(1.0, 1.0),
        );
        state.editor.selection = Some(id);
        let _ = update(&mut state, Message::DeleteSelected);
        assert!(state.document.image.annotations().is_empty());
        assert_eq!(state.editor.selection, None);
    }

    #[test]
    fn escape_priority_draft_then_selection_then_close() {
        let mut state = unsaved_workspace();
        let id = state.document.image.add_number_callout(
            ImagePoint::new(1.0, 1.0),
            ImagePoint::new(1.0, 1.0),
        );
        state.editor.selection = Some(id);
        state.editor.drag = Some(DragState::CreateRedaction {
            anchor: ImagePoint::new(0.0, 0.0),
            current: ImagePoint::new(1.0, 1.0),
        });

        let _ = update(&mut state, Message::EscapePressed);
        assert!(state.editor.drag.is_none(), "1st Esc cancels the draft");
        assert_eq!(state.editor.selection, Some(id), "selection survives");

        let _ = update(&mut state, Message::EscapePressed);
        assert_eq!(state.editor.selection, None, "2nd Esc clears selection");
        assert!(state.pending_discard.is_none());

        let _ = update(&mut state, Message::EscapePressed);
        assert!(state.pending_discard.is_some(), "3rd Esc requests close (unsaved)");
    }

    #[test]
    fn undo_after_undo_clears_selection_of_removed_annotation() {
        let mut state = unsaved_workspace();
        let id = state.document.image.add_number_callout(
            ImagePoint::new(1.0, 1.0),
            ImagePoint::new(1.0, 1.0),
        );
        state.editor.selection = Some(id);
        let _ = update(&mut state, Message::Undo);
        assert_eq!(state.editor.selection, None, "spec §15: stale selection cleared");
    }
```

- [ ] **Step 2: Run to verify failure**

Run: `rtk cargo test -p rollshot-app`
Expected: FAIL / compile errors.

- [ ] **Step 3: Implement `canvas.rs` editor state** (full replacement of the stub)

```rust
//! Editor/session state for the Result Workspace (spec §5.2/§7): active tool,
//! selection, in-progress gesture drafts, and the inline text draft. None of
//! this enters the image document or its history.

use iced::widget::text_editor;
use iced::Point;
use rollshot_image_document::{
    Annotation, AnnotationId, HitPart, ImagePoint, ImageRect, ResizeHandle,
};
use std::time::Instant;

/// Screen-space hit tolerance; divide by the viewport scale for image space.
pub const HIT_TOLERANCE_SCREEN: f32 = 8.0;
/// Screen-space slop and window for double-click detection.
pub const DOUBLE_CLICK_SLOP_SCREEN: f32 = 6.0;
pub const DOUBLE_CLICK_WINDOW_MS: u128 = 400;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tool {
    Select,
    Number,
    Text,
    Redact,
}

/// An in-progress pointer gesture. Exactly ONE document edit is submitted on
/// release (spec §5.2); previews are rendered from this state only.
#[derive(Debug, Clone)]
pub enum DragState {
    /// Select-tool drag on empty canvas: pans via the scrollable.
    Pan { last_pointer: Point },
    /// Number tool: tip anchored at the press point, bubble follows the drag.
    CreateNumber { tip: ImagePoint, bubble: ImagePoint },
    CreateRedaction { anchor: ImagePoint, current: ImagePoint },
    /// Select-tool drag of an existing annotation or one of its handles.
    EditAnnotation {
        part: HitPart,
        original: Annotation,
        /// press point − annotation reference point, so the body doesn't jump.
        grab_offset: (f32, f32),
        current: Annotation,
    },
}

/// The inline multi-line text editor draft (spec §9.3).
pub struct TextDraft {
    /// `Some(id)` when re-editing an existing note, `None` when creating.
    pub target: Option<AnnotationId>,
    pub position: ImagePoint,
    pub content: text_editor::Content,
}

pub struct EditorState {
    pub tool: Tool,
    pub selection: Option<AnnotationId>,
    pub drag: Option<DragState>,
    pub text_draft: Option<TextDraft>,
    pub navigator_open: bool,
    pub copy_menu_open: bool,
    /// Document `state_id` at the last successful Save As (dirty marker).
    pub saved_state_id: u64,
    /// Last canvas press, for double-click detection.
    pub last_press: Option<(Instant, ImagePoint)>,
}

impl EditorState {
    pub fn new(saved_state_id: u64, navigator_open: bool) -> Self {
        Self {
            tool: Tool::Select,
            selection: None,
            drag: None,
            text_draft: None,
            navigator_open,
            copy_menu_open: false,
            saved_state_id,
            last_press: None,
        }
    }
}

/// Pure drag-preview: the annotation as it would be committed if the pointer
/// released at `point`. Used by both the live draft rendering and the
/// release-commit, so preview and result cannot diverge.
pub fn dragged_annotation(
    original: &Annotation,
    part: HitPart,
    point: ImagePoint,
    grab_offset: (f32, f32),
) -> Annotation {
    let mut next = original.clone();
    match (&mut next, part) {
        (Annotation::NumberCallout { tip, .. }, HitPart::NumberTip) => *tip = point,
        (Annotation::NumberCallout { bubble, .. }, HitPart::NumberBubble) => *bubble = point,
        (Annotation::NumberCallout { tip, bubble, .. }, HitPart::Body) => {
            let dx = point.x - grab_offset.0 - bubble.x;
            let dy = point.y - grab_offset.1 - bubble.y;
            *tip = ImagePoint::new(tip.x + dx, tip.y + dy);
            *bubble = ImagePoint::new(bubble.x + dx, bubble.y + dy);
        }
        (Annotation::TextNote { position, .. }, HitPart::Body) => {
            *position = ImagePoint::new(point.x - grab_offset.0, point.y - grab_offset.1);
        }
        (Annotation::OpaqueRedaction { bounds, .. }, HitPart::Body) => {
            bounds.x = point.x - grab_offset.0;
            bounds.y = point.y - grab_offset.1;
        }
        (Annotation::OpaqueRedaction { bounds, .. }, HitPart::Resize(handle)) => {
            *bounds = resized_rect(*bounds, handle, point);
        }
        _ => {}
    }
    next
}

fn resized_rect(original: ImageRect, handle: ResizeHandle, p: ImagePoint) -> ImageRect {
    let left = original.x;
    let top = original.y;
    let right = original.x + original.width;
    let bottom = original.y + original.height;
    let (l, t, r, b) = match handle {
        ResizeHandle::TopLeft => (p.x, p.y, right, bottom),
        ResizeHandle::Top => (left, p.y, right, bottom),
        ResizeHandle::TopRight => (left, p.y, p.x, bottom),
        ResizeHandle::Right => (left, top, p.x, bottom),
        ResizeHandle::BottomRight => (left, top, p.x, p.y),
        ResizeHandle::Bottom => (left, top, right, p.y),
        ResizeHandle::BottomLeft => (p.x, top, right, p.y),
        ResizeHandle::Left => (p.x, top, right, bottom),
    };
    ImageRect::from_corners(ImagePoint::new(l, t), ImagePoint::new(r, b))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resize_from_each_side_normalizes_inverted_drags() {
        let rect = ImageRect { x: 10.0, y: 10.0, width: 20.0, height: 20.0 };
        let r = resized_rect(rect, ResizeHandle::Right, ImagePoint::new(50.0, 99.0));
        assert_eq!(r, ImageRect { x: 10.0, y: 10.0, width: 40.0, height: 20.0 });
        // Dragging the right edge past the left side flips cleanly.
        let flipped = resized_rect(rect, ResizeHandle::Right, ImagePoint::new(2.0, 99.0));
        assert_eq!(flipped, ImageRect { x: 2.0, y: 10.0, width: 8.0, height: 20.0 });
    }

    #[test]
    fn body_drag_preserves_grab_offset_and_moves_number_as_a_unit() {
        let original = Annotation::NumberCallout {
            id: AnnotationId(1),
            number: 1,
            tip: ImagePoint::new(0.0, 0.0),
            bubble: ImagePoint::new(10.0, 10.0),
        };
        let moved =
            dragged_annotation(&original, HitPart::Body, ImagePoint::new(25.0, 25.0), (5.0, 5.0));
        match moved {
            Annotation::NumberCallout { tip, bubble, .. } => {
                assert_eq!(bubble, ImagePoint::new(20.0, 20.0));
                assert_eq!(tip, ImagePoint::new(10.0, 10.0), "tip moves by the same delta");
            }
            _ => panic!(),
        }
    }
}
```

- [ ] **Step 4: Add messages and arms in `update.rs`**

New `Message` variants:

```rust
    /// Toolbar or keyboard tool selection.
    SelectTool(canvas::Tool),
    Undo,
    Redo,
    DeleteSelected,
    /// Esc with most-local-first priority (spec §9.5).
    EscapePressed,
    ToggleNavigator,
    ToggleCopyMenu,
    CopyOriginal, // added in Task 15
```

Arms:

```rust
        Message::SelectTool(tool) => {
            commit_text_draft(state);
            state.editor.tool = tool;
            state.editor.drag = None;
            Task::none()
        }
        Message::Undo => {
            commit_text_draft(state);
            state.document.image.undo();
            prune_stale_selection(state);
            Task::none()
        }
        Message::Redo => {
            commit_text_draft(state);
            state.document.image.redo();
            prune_stale_selection(state);
            Task::none()
        }
        Message::DeleteSelected => {
            if state.editor.text_draft.is_some() {
                return Task::none(); // typing in the inline editor
            }
            if let Some(id) = state.editor.selection.take() {
                let _ = state.document.image.delete_annotation(id);
            }
            Task::none()
        }
        Message::EscapePressed => {
            if state.editor.copy_menu_open {
                state.editor.copy_menu_open = false;
            } else if state.editor.text_draft.is_some() {
                state.editor.text_draft = None; // cancel without editing the document
            } else if state.editor.drag.is_some() {
                state.editor.drag = None;
            } else if state.editor.selection.is_some() {
                state.editor.selection = None;
            } else {
                return update(state, Message::RequestClose);
            }
            Task::none()
        }
        Message::ToggleNavigator => {
            state.editor.navigator_open = !state.editor.navigator_open;
            Task::none()
        }
        Message::ToggleCopyMenu => {
            state.editor.copy_menu_open = !state.editor.copy_menu_open;
            Task::none()
        }
```

Helpers (in `update.rs`):

```rust
/// Drop a selection whose annotation no longer exists (spec §15).
fn prune_stale_selection(state: &mut ResultWorkspace) {
    if let Some(id) = state.editor.selection {
        if state.document.image.annotation(id).is_none() {
            state.editor.selection = None;
        }
    }
}

/// Commit a valid inline text draft, or cancel an invalid one (spec §15).
/// Full implementation lands with the text editor task; until then:
fn commit_text_draft(state: &mut ResultWorkspace) {
    // Task 20 replaces this body. Draft-less calls are no-ops.
    let _ = state;
}
```

Also call `commit_text_draft(state)` at the top of the `Copy`, `CopyOriginal`, `SaveAs`, and `RequestClose` arms so a pending note is committed before any output or close action (spec §15: losing focus commits valid drafts).

- [ ] **Step 5: Run tests**

Run: `rtk cargo test -p rollshot-app`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
rtk git add crates/rollshot-app
rtk git commit -m "feat(app): editor state, tools, undo/redo/delete, esc priority"
```

---

### Task 17: Toolbar with icon tools and labeled output cluster (D2), Copy menu

**Files:**
- Modify: `crates/rollshot-app/src/result_workspace/view.rs`
- Modify: `crates/rollshot-app/src/result_workspace/update.rs` (tests)

- [ ] **Step 1: Write failing tests** (in `update.rs` tests — view behavior is driven by state, so test the state side; visual checks land in runtime verification)

```rust
    #[test]
    fn copy_menu_toggles_and_copy_original_closes_it() {
        let mut state = unsaved_workspace();
        let _ = update(&mut state, Message::ToggleCopyMenu);
        assert!(state.editor.copy_menu_open);
        let _ = update(&mut state, Message::CopyOriginal);
        assert!(!state.editor.copy_menu_open, "choosing an item closes the menu");
    }
```

(Adjust the `CopyOriginal` arm from Task 15 to also set `state.editor.copy_menu_open = false;`.)

- [ ] **Step 2: Run to verify failure, then make it pass**

Run: `rtk cargo test -p rollshot-app copy_menu`
Expected: FAIL → add the close-menu line → PASS.

- [ ] **Step 3: Implement the toolbar in `view.rs`**

Replace the `toolbar` row in `view()` and add helpers. D2: creation tools, Undo/Redo, Navigator are icon buttons with tooltips; Copy ▾ / Save As / Reveal keep text labels.

```rust
use iced::widget::tooltip;
use super::canvas::Tool;

/// Icon glyphs covered by the bundled DejaVu Sans font.
const ICON_SELECT: &str = "\u{2196}"; // ↖
const ICON_NUMBER: &str = "\u{2460}"; // ①
const ICON_TEXT: &str = "T";
const ICON_REDACT: &str = "\u{2588}"; // █
const ICON_UNDO: &str = "\u{21B6}"; // ↶
const ICON_REDO: &str = "\u{21B7}"; // ↷
const ICON_NAVIGATOR: &str = "\u{2261}"; // ≡

fn shortcut_label(name: &str, key: &str) -> String {
    format!("{name} ({key})")
}

fn icon_button<'a>(
    glyph: &'a str,
    tip: String,
    message: Message,
    active: bool,
) -> Element<'a, Message> {
    let btn = button(text(glyph).size(16))
        .padding([4, 10])
        .on_press(message)
        .style(if active { button::primary } else { button::secondary });
    tooltip(btn, text(tip), tooltip::Position::Bottom).into()
}

fn tool_button<'a>(
    glyph: &'a str,
    name: &str,
    key: &str,
    tool: Tool,
    state: &ResultWorkspace,
) -> Element<'a, Message> {
    icon_button(
        glyph,
        shortcut_label(name, key),
        Message::SelectTool(tool),
        state.editor.tool == tool,
    )
}

fn toolbar(state: &ResultWorkspace) -> Element<'_, Message> {
    let undo_btn = button(text(ICON_UNDO).size(16))
        .padding([4, 10])
        .on_press_maybe(state.document.image.can_undo().then_some(Message::Undo));
    let redo_btn = button(text(ICON_REDO).size(16))
        .padding([4, 10])
        .on_press_maybe(state.document.image.can_redo().then_some(Message::Redo));

    row![
        button(text("Close")).on_press(Message::RequestClose),
        text(state.document.display_name()).width(Length::Fill),
        tool_button(ICON_SELECT, "Select", "V", Tool::Select, state),
        tool_button(ICON_NUMBER, "Number", "N", Tool::Number, state),
        tool_button(ICON_TEXT, "Text", "T", Tool::Text, state),
        tool_button(ICON_REDACT, "Redact", "R", Tool::Redact, state),
        tooltip(undo_btn, text(shortcut_label("Undo", "Ctrl+Z")), tooltip::Position::Bottom),
        tooltip(
            redo_btn,
            text(shortcut_label("Redo", "Ctrl+Shift+Z")),
            tooltip::Position::Bottom
        ),
        icon_button(
            ICON_NAVIGATOR,
            "Navigator".to_string(),
            Message::ToggleNavigator,
            state.editor.navigator_open,
        ),
        // D2: trust-bearing output actions keep visible text labels.
        button(text("Copy")).on_press(Message::Copy),
        button(text("\u{25BE}")).on_press(Message::ToggleCopyMenu), // ▾
        button(text("Save As")).on_press(Message::SaveAs),
        reveal_button(state),
    ]
    .spacing(8)
    .align_y(Alignment::Center)
    .into()
}
```

On macOS the tooltip shortcut text should read Cmd: compute `let modifier = if cfg!(target_os = "macos") { "Cmd" } else { "Ctrl" };` and format Undo/Redo tips from it.

- [ ] **Step 4: Copy menu overlay**

In `view()` after the discard-modal stacking, add a second conditional layer (same scrim pattern as `discard_modal` — see the levitation comment there):

```rust
fn copy_menu(base: Element<'_, Message>) -> Element<'_, Message> {
    let menu = container(
        button(text("Copy Original")).on_press(Message::CopyOriginal),
    )
    .padding(4);

    // Anchored near the toolbar's output cluster (top-right). Outside clicks
    // close the menu without copying.
    let positioned = container(menu)
        .width(Length::Fill)
        .height(Length::Fill)
        .align_x(Alignment::End)
        .padding(iced::Padding { top: 44.0, right: 180.0, ..Default::default() });

    let scrim = mouse_area(positioned)
        .interaction(mouse::Interaction::Idle)
        .on_press(Message::ToggleCopyMenu);

    iced::widget::stack![base, scrim].into()
}
```

Wire in `view()`:

```rust
    let layout: Element<'_, Message> = if let Some(prompt) = &state.pending_discard {
        discard_modal(layout, prompt.text())
    } else {
        layout.into()
    };
    if state.editor.copy_menu_open {
        copy_menu(layout)
    } else {
        layout
    }
```

(`discard_modal` gains the `&str` prompt parameter from Task 14.)

Note: the scrim's `on_press` fires for the press on the menu button area as well in this simple pattern only if the button doesn't capture it first — iced buttons capture their presses, so "Copy Original" wins; an outside press reaches the scrim and closes. This mirrors the verified `discard_modal` levitation behavior.

- [ ] **Step 5: Verify build + tests**

Run: `rtk cargo test -p rollshot-app && rtk cargo clippy -p rollshot-app --all-targets -- -D warnings`
Expected: PASS / clean.

- [ ] **Step 6: Commit**

```bash
rtk git add crates/rollshot-app
rtk git commit -m "feat(app): annotation toolbar with icon tools, labeled output cluster, copy menu"
```

---

### Task 18: Annotation canvas overlay — drawing, culling, event translation

The canvas is stacked exactly over the image widget inside the existing scrollable, so canvas-local coordinates ÷ scale ARE image coordinates. It translates pointer events into image-space messages; all gesture logic stays in `update.rs` (Task 19) where it is unit-testable.

**Files:**
- Modify: `crates/rollshot-app/src/result_workspace/canvas.rs`
- Modify: `crates/rollshot-app/src/result_workspace/view.rs` (stack into `canvas_view`)
- Modify: `crates/rollshot-app/src/result_workspace/update.rs` (message variants + visible-rect helper)

- [ ] **Step 1: Write failing tests** (in `canvas.rs` tests)

```rust
    use rollshot_image_document::{annotation_bounds, ImageDocument};
    use image::{Rgba, RgbaImage};

    #[test]
    fn visible_image_rect_maps_scroll_and_scale() {
        // 100×200 image at scale 2 → rendered 200×400. Viewport 50×80 scrolled
        // to (20, 40) sees image rect (10, 20, 25, 40).
        let visible = visible_image_rect(
            iced::Vector::new(20.0, 40.0),
            iced::Size::new(50.0, 80.0),
            2.0,
            iced::Point::new(0.0, 0.0),
        );
        assert_eq!(visible, ImageRect { x: 10.0, y: 20.0, width: 25.0, height: 40.0 });
    }

    #[test]
    fn culling_skips_annotations_outside_the_visible_rect() {
        let mut doc = ImageDocument::new(RgbaImage::from_pixel(
            100,
            10000,
            Rgba([0, 0, 0, 255]),
        ));
        let near = doc.add_number_callout(ImagePoint::new(50.0, 50.0), ImagePoint::new(50.0, 50.0));
        let far = doc.add_number_callout(
            ImagePoint::new(50.0, 9000.0),
            ImagePoint::new(50.0, 9000.0),
        );
        let visible = ImageRect { x: 0.0, y: 0.0, width: 100.0, height: 200.0 };
        let drawn: Vec<_> = doc
            .annotations()
            .iter()
            .filter(|a| annotation_bounds(a).intersects(&visible))
            .map(|a| a.id())
            .collect();
        assert_eq!(drawn, vec![near]);
        assert!(!drawn.contains(&far));
    }
```

- [ ] **Step 2: Run to verify failure**

Run: `rtk cargo test -p rollshot-app visible_image`
Expected: FAIL — `visible_image_rect` not defined.

- [ ] **Step 3: Implement the canvas program** (append to `canvas.rs`)

```rust
use iced::widget::canvas;
use iced::{mouse, Color, Point, Rectangle, Renderer, Size, Theme, Vector};
use rollshot_image_document::{
    annotation_bounds, annotation_shapes, redaction_handles, style, Annotation, ImageRect,
    RenderShape, TextAnchor,
};

use super::update::Message;

/// Screen-space radius of selection handles (zoom-independent).
pub const HANDLE_RADIUS_SCREEN: f32 = 6.0;

pub(crate) fn token_color(c: rollshot_image_document::Rgba8) -> Color {
    Color::from_rgba8(c.r, c.g, c.b, c.a as f32 / 255.0)
}

const ANNOTATION_FONT: iced::Font = iced::Font::with_name(style::FONT_FAMILY_NAME);
const ANNOTATION_FONT_BOLD: iced::Font = iced::Font {
    weight: iced::font::Weight::Bold,
    ..iced::Font::with_name(style::FONT_FAMILY_NAME)
};

/// The portion of the image (image coordinates) currently visible in the
/// scrollable viewport — culling input (spec §11.1).
pub fn visible_image_rect(
    scroll_offset: Vector,
    viewport: Size,
    scale: f32,
    image_origin: Point,
) -> ImageRect {
    ImageRect {
        x: (scroll_offset.x - image_origin.x) / scale,
        y: (scroll_offset.y - image_origin.y) / scale,
        width: viewport.width / scale,
        height: viewport.height / scale,
    }
}

/// View-built canvas program: draws committed annotations (culled), the
/// active draft, and selection handles; translates pointer events into
/// image-space messages. All state lives in `ResultWorkspace`.
pub struct AnnotationCanvas<'a> {
    pub document: &'a rollshot_image_document::ImageDocument,
    pub editor: &'a EditorState,
    pub scale: f32,
    pub visible: ImageRect,
}

impl AnnotationCanvas<'_> {
    fn image_point(&self, local: Point) -> ImagePoint {
        ImagePoint::new(local.x / self.scale, local.y / self.scale)
    }

    /// The annotation id whose committed visual is replaced by a draft.
    fn dragged_id(&self) -> Option<AnnotationId> {
        match &self.editor.drag {
            Some(DragState::EditAnnotation { original, .. }) => Some(original.id()),
            _ => None,
        }
    }

    fn draw_shape(&self, frame: &mut canvas::Frame, shape: &RenderShape) {
        let s = self.scale;
        match shape {
            RenderShape::Rect { rect, color } => frame.fill_rectangle(
                Point::new(rect.x * s, rect.y * s),
                Size::new(rect.width * s, rect.height * s),
                token_color(*color),
            ),
            RenderShape::Circle { center, radius, fill, outline_width, outline } => {
                let c = Point::new(center.x * s, center.y * s);
                frame.fill(&canvas::Path::circle(c, radius * s), token_color(*fill));
                if *outline_width > 0.0 {
                    frame.stroke(
                        &canvas::Path::circle(c, radius * s),
                        canvas::Stroke::default()
                            .with_color(token_color(*outline))
                            .with_width(outline_width * s),
                    );
                }
            }
            RenderShape::Triangle { points, color } => {
                let path = canvas::Path::new(|b| {
                    b.move_to(Point::new(points[0].x * s, points[0].y * s));
                    b.line_to(Point::new(points[1].x * s, points[1].y * s));
                    b.line_to(Point::new(points[2].x * s, points[2].y * s));
                    b.close();
                });
                frame.fill(&path, token_color(*color));
            }
            RenderShape::Label { anchor, anchor_kind, content, px, bold, color } => {
                let (align_x, align_y) = match anchor_kind {
                    TextAnchor::Center => (
                        iced::widget::text::Alignment::Center,
                        iced::alignment::Vertical::Center,
                    ),
                    TextAnchor::TopLeft => (
                        iced::widget::text::Alignment::Default,
                        iced::alignment::Vertical::Top,
                    ),
                };
                frame.fill_text(canvas::Text {
                    content: content.clone(),
                    position: Point::new(anchor.x * s, anchor.y * s),
                    color: token_color(*color),
                    size: iced::Pixels(px * s),
                    line_height: iced::widget::text::LineHeight::Relative(
                        style::TEXT_LINE_HEIGHT,
                    ),
                    font: if *bold { ANNOTATION_FONT_BOLD } else { ANNOTATION_FONT },
                    align_x,
                    align_y,
                    ..canvas::Text::default()
                });
            }
        }
    }

    fn draw_annotation(&self, frame: &mut canvas::Frame, annotation: &Annotation) {
        for shape in annotation_shapes(annotation) {
            self.draw_shape(frame, &shape);
        }
    }

    fn draft_annotation(&self) -> Option<Annotation> {
        match &self.editor.drag {
            Some(DragState::CreateNumber { tip, bubble }) => Some(Annotation::NumberCallout {
                id: AnnotationId(u64::MAX), // draft-only, never committed
                number: self.document.next_number(),
                tip: *tip,
                bubble: *bubble,
            }),
            Some(DragState::CreateRedaction { anchor, current }) => {
                let rect = ImageRect::from_corners(*anchor, *current);
                (!rect.is_empty()).then_some(Annotation::OpaqueRedaction {
                    id: AnnotationId(u64::MAX),
                    bounds: rect,
                })
            }
            Some(DragState::EditAnnotation { current, .. }) => Some(current.clone()),
            _ => None,
        }
    }

    fn draw_selection_handles(&self, frame: &mut canvas::Frame, annotation: &Annotation) {
        let s = self.scale;
        let accent = token_color(style::ACCENT);
        let white = token_color(style::WHITE);
        let handle = |frame: &mut canvas::Frame, p: ImagePoint, fill: Color, ring: Color| {
            let c = Point::new(p.x * s, p.y * s);
            frame.fill(&canvas::Path::circle(c, HANDLE_RADIUS_SCREEN), fill);
            frame.stroke(
                &canvas::Path::circle(c, HANDLE_RADIUS_SCREEN),
                canvas::Stroke::default().with_color(ring).with_width(2.0),
            );
        };
        match annotation {
            Annotation::NumberCallout { tip, bubble, .. } => {
                // mark-shot convention: accent bubble handle, white tip handle.
                handle(frame, *bubble, accent, white);
                handle(frame, *tip, white, accent);
            }
            Annotation::TextNote { position, text, .. } => {
                let plate = rollshot_image_document::text_plate_rect(*position, text);
                frame.stroke(
                    &canvas::Path::rectangle(
                        Point::new(plate.x * s, plate.y * s),
                        Size::new(plate.width * s, plate.height * s),
                    ),
                    canvas::Stroke::default().with_color(accent).with_width(2.0),
                );
            }
            Annotation::OpaqueRedaction { bounds, .. } => {
                for (_, p) in redaction_handles(*bounds) {
                    handle(frame, p, white, accent);
                }
            }
        }
    }
}

impl canvas::Program<Message> for AnnotationCanvas<'_> {
    type State = ();

    fn update(
        &self,
        _state: &mut (),
        event: &canvas::Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<canvas::Action<Message>> {
        match event {
            iced::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                let local = cursor.position_in(bounds)?;
                Some(
                    canvas::Action::publish(Message::CanvasPressed(self.image_point(local)))
                        .and_capture(),
                )
            }
            iced::Event::Mouse(mouse::Event::CursorMoved { .. }) => {
                let position = cursor.position()?;
                let local = Point::new(position.x - bounds.x, position.y - bounds.y);
                Some(canvas::Action::publish(Message::CanvasMoved(self.image_point(local))))
            }
            iced::Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                // Releases outside the canvas still end the gesture.
                let position = cursor.position()?;
                let local = Point::new(position.x - bounds.x, position.y - bounds.y);
                Some(canvas::Action::publish(Message::CanvasReleased(self.image_point(local))))
            }
            _ => None,
        }
    }

    fn draw(
        &self,
        _state: &(),
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let mut frame = canvas::Frame::new(renderer, bounds.size());
        let dragged = self.dragged_id();
        let editing_text = self.editor.text_draft.as_ref().and_then(|d| d.target);

        // Committed annotations, culled against the visible viewport
        // (spec §11.1). The drag target and the re-edited note render as
        // draft/editor instead.
        for annotation in self.document.annotations() {
            if Some(annotation.id()) == dragged || Some(annotation.id()) == editing_text {
                continue;
            }
            if annotation_bounds(annotation).intersects(&self.visible) {
                self.draw_annotation(&mut frame, annotation);
            }
        }

        if let Some(draft) = self.draft_annotation() {
            self.draw_annotation(&mut frame, &draft);
        }

        if let Some(id) = self.editor.selection {
            if Some(id) != dragged && Some(id) != editing_text {
                if let Some(annotation) = self.document.annotation(id) {
                    self.draw_selection_handles(&mut frame, annotation);
                }
            } else if let Some(draft) = self.draft_annotation() {
                self.draw_selection_handles(&mut frame, &draft);
            }
        }

        vec![frame.into_geometry()]
    }

    fn mouse_interaction(
        &self,
        _state: &(),
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> mouse::Interaction {
        let Some(local) = cursor.position_in(bounds) else {
            return mouse::Interaction::default();
        };
        match self.editor.tool {
            // Spec §7 hover/handle feedback: stateless hover via hit-testing
            // the live cursor — grab over bodies/handles, default elsewhere.
            Tool::Select => {
                let tolerance = HIT_TOLERANCE_SCREEN / self.scale;
                match self.document.hit_test(self.image_point(local), tolerance) {
                    Some(hit) => match hit.part {
                        rollshot_image_document::HitPart::Resize(_) => {
                            mouse::Interaction::Crosshair
                        }
                        _ => mouse::Interaction::Grab,
                    },
                    None => mouse::Interaction::default(),
                }
            }
            _ => mouse::Interaction::Crosshair,
        }
    }
}
```

Add the message variants in `update.rs`:

```rust
    /// Canvas pointer events in image coordinates (from AnnotationCanvas).
    CanvasPressed(rollshot_image_document::ImagePoint),
    CanvasMoved(rollshot_image_document::ImagePoint),
    CanvasReleased(rollshot_image_document::ImagePoint),
```

with placeholder arms `=> Task::none()` (Task 19 implements them; the enum must compile now).

- [ ] **Step 4: Stack the canvas over the image in `view.rs` `canvas_view`**

Replace the `let content = container(img)...` block:

```rust
    let overlay = iced::widget::canvas(super::canvas::AnnotationCanvas {
        document: &state.document.image,
        editor: &state.editor,
        scale: geometry.scale,
        visible: super::canvas::visible_image_rect(
            state.viewport.scroll_offset,
            state.viewport_bounds,
            geometry.scale,
            geometry.image_origin,
        ),
    })
    .width(Length::Fixed(geometry.rendered_size.width))
    .height(Length::Fixed(geometry.rendered_size.height));

    let layered = iced::widget::stack![img, overlay];

    let content = container(layered)
        .width(Length::Fixed(geometry.content_size.width))
        .height(Length::Fixed(geometry.content_size.height))
        .padding(iced::Padding {
            left: geometry.image_origin.x,
            top: geometry.image_origin.y,
            right: 0.0,
            bottom: 0.0,
        });
```

`visible_image_rect` takes the origin so a centered (non-overflowing) image culls correctly; with the canvas positioned at the origin via the container padding, the offsets cancel — keep the helper signature as implemented and pass `geometry.image_origin` as shown.

- [ ] **Step 5: Run tests and build**

Run: `rtk cargo test -p rollshot-app && rtk cargo build -p rollshot-app`
Expected: PASS / builds.

- [ ] **Step 6: Commit**

```bash
rtk git add crates/rollshot-app
rtk git commit -m "feat(app): annotation canvas overlay with culling and event translation"
```

---

### Task 19: Tool gestures — create, move, resize, pan, click-vs-drag

**Files:**
- Modify: `crates/rollshot-app/src/result_workspace/update.rs`

- [ ] **Step 1: Write failing tests** (in `update.rs` tests)

```rust
    fn press_move_release(
        state: &mut ResultWorkspace,
        from: ImagePoint,
        to: ImagePoint,
    ) {
        let _ = update(state, Message::CanvasPressed(from));
        let _ = update(state, Message::CanvasMoved(to));
        let _ = update(state, Message::CanvasReleased(to));
    }

    #[test]
    fn number_click_creates_coincident_stamp_and_keeps_tool_active() {
        let mut state = unsaved_workspace();
        let _ = update(&mut state, Message::SelectTool(Tool::Number));
        let p = ImagePoint::new(1.0, 1.0);
        press_move_release(&mut state, p, p);
        match &state.document.image.annotations()[0] {
            rollshot_image_document::Annotation::NumberCallout { tip, bubble, number, .. } => {
                assert_eq!(tip, bubble, "click → coincident stamp");
                assert_eq!(*number, 1);
            }
            _ => panic!(),
        }
        assert_eq!(state.editor.tool, Tool::Number, "spec §9.2: tool stays active");
        // Consecutive callout increments the sequence.
        press_move_release(&mut state, ImagePoint::new(1.5, 1.5), ImagePoint::new(1.5, 1.5));
        assert_eq!(state.document.image.next_number(), 3);
    }

    #[test]
    fn number_drag_anchors_tip_and_separates_bubble_in_one_edit() {
        let mut state = unsaved_workspace();
        let _ = update(&mut state, Message::SelectTool(Tool::Number));
        press_move_release(&mut state, ImagePoint::new(0.5, 0.5), ImagePoint::new(1.8, 1.8));
        assert_eq!(state.document.image.annotations().len(), 1);
        match &state.document.image.annotations()[0] {
            rollshot_image_document::Annotation::NumberCallout { tip, bubble, .. } => {
                assert_eq!(*tip, ImagePoint::new(0.5, 0.5), "tip anchored at press");
                assert_eq!(*bubble, ImagePoint::new(1.8, 1.8), "bubble follows drag");
            }
            _ => panic!(),
        }
        let mut undo_steps = 0;
        while state.document.image.undo() {
            undo_steps += 1;
        }
        assert_eq!(undo_steps, 1, "spec §5.2: one drag = one history entry");
    }

    #[test]
    fn redaction_drag_creates_rect_and_zero_drag_creates_nothing() {
        let mut state = unsaved_workspace();
        let _ = update(&mut state, Message::SelectTool(Tool::Redact));
        press_move_release(&mut state, ImagePoint::new(0.0, 0.0), ImagePoint::new(2.0, 2.0));
        assert_eq!(state.document.image.annotations().len(), 1);
        // A click (zero-area) commits nothing (spec §6).
        press_move_release(&mut state, ImagePoint::new(1.0, 1.0), ImagePoint::new(1.0, 1.0));
        assert_eq!(state.document.image.annotations().len(), 1);
    }

    #[test]
    fn select_click_on_annotation_selects_without_history_entry() {
        let mut state = unsaved_workspace();
        let id = state.document.image.add_number_callout(
            ImagePoint::new(1.0, 1.0),
            ImagePoint::new(1.0, 1.0),
        );
        let s = state.document.image.state_id();
        let _ = update(&mut state, Message::SelectTool(Tool::Select));
        press_move_release(&mut state, ImagePoint::new(1.0, 1.0), ImagePoint::new(1.0, 1.0));
        assert_eq!(state.editor.selection, Some(id));
        assert_eq!(state.document.image.state_id(), s, "no-move release edits nothing");
    }

    #[test]
    fn select_click_on_empty_canvas_clears_selection_without_edits() {
        let mut state = workspace_with_size(100, 100); // helper: image 100×100
        let id = state.document.image.add_number_callout(
            ImagePoint::new(10.0, 10.0),
            ImagePoint::new(10.0, 10.0),
        );
        state.editor.selection = Some(id);
        let s = state.document.image.state_id();
        press_move_release(&mut state, ImagePoint::new(90.0, 90.0), ImagePoint::new(90.0, 90.0));
        assert_eq!(state.editor.selection, None);
        assert_eq!(state.document.image.state_id(), s);
    }

    #[test]
    fn dragging_the_bubble_commits_one_set_points_edit() {
        let mut state = workspace_with_size(200, 200);
        let id = state.document.image.add_number_callout(
            ImagePoint::new(20.0, 20.0),
            ImagePoint::new(100.0, 100.0),
        );
        press_move_release(&mut state, ImagePoint::new(100.0, 100.0), ImagePoint::new(150.0, 150.0));
        match state.document.image.annotation(id).unwrap() {
            rollshot_image_document::Annotation::NumberCallout { tip, bubble, .. } => {
                assert_eq!(*bubble, ImagePoint::new(150.0, 150.0));
                assert_eq!(*tip, ImagePoint::new(20.0, 20.0), "tip moves independently");
            }
            _ => panic!(),
        }
    }

    #[test]
    fn resizing_a_redaction_commits_new_bounds() {
        let mut state = workspace_with_size(200, 200);
        let id = state
            .document
            .image
            .add_redaction(ImageRect { x: 50.0, y: 50.0, width: 40.0, height: 30.0 })
            .unwrap();
        // Grab the bottom-right corner handle and drag outward.
        press_move_release(&mut state, ImagePoint::new(90.0, 80.0), ImagePoint::new(120.0, 110.0));
        match state.document.image.annotation(id).unwrap() {
            rollshot_image_document::Annotation::OpaqueRedaction { bounds, .. } => {
                assert_eq!(*bounds, ImageRect { x: 50.0, y: 50.0, width: 70.0, height: 60.0 });
            }
            _ => panic!(),
        }
    }
```

Add the test helper next to `workspace()`:

```rust
    fn workspace_with_size(w: u32, h: u32) -> ResultWorkspace {
        let img = RgbaImage::from_pixel(w, h, Rgba([100, 150, 200, 255]));
        let mut ws = ResultWorkspace::new(ResultDocument::unsaved(img), None);
        // 1:1 scale so test image points equal screen points.
        ws.viewport.zoom = ZoomMode::ActualSize;
        ws.apply_viewport_bounds(Size::new(w as f32, h as f32));
        ws
    }
```

- [ ] **Step 2: Run to verify failure**

Run: `rtk cargo test -p rollshot-app number_click`
Expected: FAIL — placeholder arms do nothing.

- [ ] **Step 3: Implement the gesture handlers** (`update.rs`)

```rust
use super::canvas::{
    dragged_annotation, DragState, EditorState, TextDraft, Tool, DOUBLE_CLICK_SLOP_SCREEN,
    DOUBLE_CLICK_WINDOW_MS, HIT_TOLERANCE_SCREEN,
};
use rollshot_image_document::{Annotation, HitPart, ImagePoint, ImageRect};

/// Current viewport scale: image px → screen px.
fn current_scale(state: &ResultWorkspace) -> f32 {
    geometry_for(state.viewport.zoom, state_original_size(state), state.viewport_bounds).scale
}

fn state_original_size(state: &ResultWorkspace) -> Size {
    let (w, h) = state.document.image.source().dimensions();
    Size::new(w as f32, h as f32)
}

fn grab_offset(annotation: &Annotation, part: HitPart, point: ImagePoint) -> (f32, f32) {
    match (annotation, part) {
        (Annotation::TextNote { position, .. }, HitPart::Body) => {
            (point.x - position.x, point.y - position.y)
        }
        (Annotation::OpaqueRedaction { bounds, .. }, HitPart::Body) => {
            (point.x - bounds.x, point.y - bounds.y)
        }
        (Annotation::NumberCallout { bubble, .. }, HitPart::Body) => {
            (point.x - bubble.x, point.y - bubble.y)
        }
        _ => (0.0, 0.0),
    }
}

pub(crate) fn handle_canvas_pressed(
    state: &mut ResultWorkspace,
    point: ImagePoint,
    now: std::time::Instant,
) -> Task<Message> {
    // Clicking outside an open inline editor commits it first (spec §9.3).
    commit_text_draft(state);
    state.editor.copy_menu_open = false;

    let scale = current_scale(state);
    let tolerance = HIT_TOLERANCE_SCREEN / scale;

    let double_click = state.editor.last_press.is_some_and(|(at, p)| {
        now.duration_since(at).as_millis() <= DOUBLE_CLICK_WINDOW_MS
            && p.distance(point) <= DOUBLE_CLICK_SLOP_SCREEN / scale
    });
    state.editor.last_press = Some((now, point));

    match state.editor.tool {
        Tool::Select => {
            if double_click {
                if let Some(hit) = state.document.image.hit_test(point, tolerance) {
                    if let Some(Annotation::TextNote { position, text, .. }) =
                        state.document.image.annotation(hit.id).cloned().as_ref()
                    {
                        // Double-click re-edits the note inline (spec §9.3).
                        state.editor.drag = None;
                        state.editor.selection = Some(hit.id);
                        state.editor.text_draft = Some(TextDraft {
                            target: Some(hit.id),
                            position: *position,
                            content: iced::widget::text_editor::Content::with_text(text),
                        });
                        return iced::widget::operation::focus(state.text_editor_id.clone());
                    }
                }
            }
            match state.document.image.hit_test(point, tolerance) {
                Some(hit) => {
                    let original = state
                        .document
                        .image
                        .annotation(hit.id)
                        .expect("hit returns existing annotations")
                        .clone();
                    state.editor.selection = Some(hit.id);
                    state.editor.drag = Some(DragState::EditAnnotation {
                        part: hit.part,
                        grab_offset: grab_offset(&original, hit.part, point),
                        current: original.clone(),
                        original,
                    });
                }
                None => {
                    // Empty canvas: clear selection (no document edit) and pan.
                    state.editor.selection = None;
                    state.editor.drag = Some(DragState::Pan {
                        last_pointer: state.pointer_position,
                    });
                }
            }
            Task::none()
        }
        Tool::Number => {
            state.editor.drag = Some(DragState::CreateNumber { tip: point, bubble: point });
            Task::none()
        }
        Tool::Text => {
            state.editor.text_draft = Some(TextDraft {
                target: None,
                position: point,
                content: iced::widget::text_editor::Content::new(),
            });
            iced::widget::operation::focus(state.text_editor_id.clone())
        }
        Tool::Redact => {
            state.editor.drag = Some(DragState::CreateRedaction { anchor: point, current: point });
            Task::none()
        }
    }
}

pub(crate) fn handle_canvas_moved(state: &mut ResultWorkspace, point: ImagePoint) -> Task<Message> {
    let (w, h) = state.document.image.source().dimensions();
    let point = point.clamp_to(w, h);
    match &mut state.editor.drag {
        Some(DragState::CreateNumber { bubble, .. }) => {
            *bubble = point;
            Task::none()
        }
        Some(DragState::CreateRedaction { current, .. }) => {
            *current = point;
            Task::none()
        }
        Some(DragState::EditAnnotation { part, original, grab_offset, current }) => {
            *current = dragged_annotation(original, *part, point, *grab_offset);
            Task::none()
        }
        Some(DragState::Pan { last_pointer }) => {
            // Pan uses the scroll-independent scrollable-local pointer.
            let pointer = state.pointer_position;
            let delta = iced::Vector::new(pointer.x - last_pointer.x, pointer.y - last_pointer.y);
            *last_pointer = pointer;
            iced::widget::operation::scroll_by(
                state.scrollable_id.clone(),
                scrollable::AbsoluteOffset { x: -delta.x, y: -delta.y },
            )
        }
        None => Task::none(),
    }
}

pub(crate) fn handle_canvas_released(
    state: &mut ResultWorkspace,
    point: ImagePoint,
) -> Task<Message> {
    let (w, h) = state.document.image.source().dimensions();
    let point = point.clamp_to(w, h);
    match state.editor.drag.take() {
        Some(DragState::CreateNumber { tip, .. }) => {
            // Click → coincident stamp; drag → separated (spec §9.2). Either
            // way exactly ONE document edit.
            let id = state.document.image.add_number_callout(tip, point);
            state.editor.selection = Some(id);
        }
        Some(DragState::CreateRedaction { anchor, .. }) => {
            // Zero-area (click) is rejected by the document and creates
            // nothing — intentionally silent.
            if let Ok(id) = state
                .document
                .image
                .add_redaction(ImageRect::from_corners(anchor, point))
            {
                state.editor.selection = Some(id);
            }
        }
        Some(DragState::EditAnnotation { original, current, .. }) => {
            if current != original {
                let result = match &current {
                    Annotation::NumberCallout { tip, bubble, .. } => state
                        .document
                        .image
                        .set_number_points(original.id(), *tip, *bubble),
                    Annotation::TextNote { position, .. } => {
                        state.document.image.set_text_position(original.id(), *position)
                    }
                    Annotation::OpaqueRedaction { bounds, .. } => {
                        state.document.image.set_redaction_bounds(original.id(), *bounds)
                    }
                };
                if let Err(e) = result {
                    state.message = Some(InlineMessage::Error(e.to_string()));
                }
            }
        }
        Some(DragState::Pan { .. }) | None => {}
    }
    Task::none()
}
```

Wire the arms:

```rust
        Message::CanvasPressed(point) => {
            handle_canvas_pressed(state, point, std::time::Instant::now())
        }
        Message::CanvasMoved(point) => handle_canvas_moved(state, point),
        Message::CanvasReleased(point) => handle_canvas_released(state, point),
```

- [ ] **Step 4: Run tests**

Run: `rtk cargo test -p rollshot-app`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
rtk git add crates/rollshot-app
rtk git commit -m "feat(app): tool gestures with one-edit-per-drag commits"
```

---

### Task 20: Inline text editor

**Files:**
- Modify: `crates/rollshot-app/src/result_workspace/update.rs` (real `commit_text_draft`, actions)
- Modify: `crates/rollshot-app/src/result_workspace/view.rs` (editor overlay in the canvas stack)

- [ ] **Step 1: Write failing tests** (in `update.rs` tests)

```rust
    fn type_text(state: &mut ResultWorkspace, s: &str) {
        if let Some(draft) = &mut state.editor.text_draft {
            for ch in s.chars() {
                draft.content.perform(iced::widget::text_editor::Action::Edit(
                    iced::widget::text_editor::Edit::Insert(ch),
                ));
            }
        }
    }

    #[test]
    fn typing_then_commit_creates_exactly_one_edit() {
        let mut state = workspace_with_size(200, 100);
        let _ = update(&mut state, Message::SelectTool(Tool::Text));
        let _ = update(&mut state, Message::CanvasPressed(ImagePoint::new(10.0, 10.0)));
        assert!(state.editor.text_draft.is_some());
        type_text(&mut state, "hello");
        let _ = update(&mut state, Message::CommitTextDraft);
        assert!(state.editor.text_draft.is_none());
        match &state.document.image.annotations()[0] {
            rollshot_image_document::Annotation::TextNote { text, .. } => {
                assert_eq!(text, "hello")
            }
            _ => panic!(),
        }
        let mut undo_steps = 0;
        while state.document.image.undo() {
            undo_steps += 1;
        }
        assert_eq!(undo_steps, 1, "spec §9.3: whole text = one undo entry");
    }

    #[test]
    fn empty_draft_commit_creates_nothing_and_esc_cancels() {
        let mut state = workspace_with_size(200, 100);
        let _ = update(&mut state, Message::SelectTool(Tool::Text));
        let _ = update(&mut state, Message::CanvasPressed(ImagePoint::new(10.0, 10.0)));
        let _ = update(&mut state, Message::CommitTextDraft);
        assert!(state.document.image.annotations().is_empty(), "spec §15");

        let _ = update(&mut state, Message::CanvasPressed(ImagePoint::new(10.0, 10.0)));
        type_text(&mut state, "draft");
        let _ = update(&mut state, Message::EscapePressed);
        assert!(state.editor.text_draft.is_none(), "esc cancels the draft");
        assert!(state.document.image.annotations().is_empty());
    }

    #[test]
    fn clicking_outside_commits_the_open_draft() {
        let mut state = workspace_with_size(200, 100);
        let _ = update(&mut state, Message::SelectTool(Tool::Text));
        let _ = update(&mut state, Message::CanvasPressed(ImagePoint::new(10.0, 10.0)));
        type_text(&mut state, "note");
        // Next canvas press commits the previous draft, then opens a new one.
        let _ = update(&mut state, Message::CanvasPressed(ImagePoint::new(100.0, 50.0)));
        assert_eq!(state.document.image.annotations().len(), 1);
    }

    #[test]
    fn double_click_reedit_commits_one_changed_text_edit() {
        let mut state = workspace_with_size(300, 100);
        let id = state
            .document
            .image
            .add_text_note(ImagePoint::new(10.0, 10.0), "old".to_string())
            .unwrap();
        let _ = update(&mut state, Message::SelectTool(Tool::Select));
        // Two presses within the double-click window on the note body.
        let now = std::time::Instant::now();
        let _ = handle_canvas_pressed(&mut state, ImagePoint::new(15.0, 15.0), now);
        let _ = handle_canvas_pressed(
            &mut state,
            ImagePoint::new(15.0, 15.0),
            now + std::time::Duration::from_millis(100),
        );
        let draft = state.editor.text_draft.as_ref().expect("re-edit draft open");
        assert_eq!(draft.target, Some(id));
        assert_eq!(draft.content.text().trim_end(), "old");
        // Replace content and commit.
        state.editor.text_draft.as_mut().unwrap().content =
            iced::widget::text_editor::Content::with_text("new");
        let _ = update(&mut state, Message::CommitTextDraft);
        match state.document.image.annotation(id).unwrap() {
            rollshot_image_document::Annotation::TextNote { text, .. } => assert_eq!(text, "new"),
            _ => panic!(),
        }
    }
```

- [ ] **Step 2: Run to verify failure**

Run: `rtk cargo test -p rollshot-app typing_then`
Expected: FAIL — `CommitTextDraft` missing, `commit_text_draft` is a stub.

- [ ] **Step 3: Implement** (`update.rs`)

Add messages:

```rust
    /// Inline text editor actions (typing, cursor moves).
    TextDraftAction(iced::widget::text_editor::Action),
    /// Ctrl/Cmd+Enter or click-outside: commit the inline draft.
    CommitTextDraft,
```

Arms:

```rust
        Message::TextDraftAction(action) => {
            if let Some(draft) = &mut state.editor.text_draft {
                draft.content.perform(action);
            }
            Task::none()
        }
        Message::CommitTextDraft => {
            commit_text_draft(state);
            Task::none()
        }
```

Replace the Task 16 stub:

```rust
/// Commit a valid inline draft as exactly one document edit; cancel an
/// empty/unchanged one without touching the document (spec §9.3, §15).
fn commit_text_draft(state: &mut ResultWorkspace) {
    let Some(draft) = state.editor.text_draft.take() else {
        return;
    };
    let text = draft.content.text().trim_end().to_string();
    match draft.target {
        None => {
            // EmptyText rejection = silent cancel.
            if let Ok(id) = state.document.image.add_text_note(draft.position, text) {
                state.editor.selection = Some(id);
            }
        }
        Some(id) => {
            // Unchanged text is a documented no-op; empty cancels the re-edit.
            let _ = state.document.image.set_text(id, text);
        }
    }
}
```

- [ ] **Step 4: Render the editor overlay** (`view.rs`, inside `canvas_view` after the canvas stack)

```rust
    let layered: Element<'_, Message> = if let Some(draft) = &state.editor.text_draft {
        let editor = iced::widget::text_editor(&draft.content)
            .id(state.text_editor_id.clone())
            .on_action(Message::TextDraftAction)
            .key_binding(|key_press| {
                use iced::widget::text_editor::{Binding, KeyPress};
                let KeyPress { key, modifiers, .. } = &key_press;
                let commit_modifier = if cfg!(target_os = "macos") {
                    modifiers.command()
                } else {
                    modifiers.control()
                };
                if commit_modifier
                    && matches!(key, keyboard::Key::Named(keyboard::key::Named::Enter))
                {
                    return Some(Binding::Custom(Message::CommitTextDraft));
                }
                Binding::from_key_press(key_press)
            })
            .font(super::canvas::annotation_font())
            .width(Length::Fixed(280.0));

        let positioned = container(editor).padding(iced::Padding {
            left: draft.position.x * geometry.scale,
            top: draft.position.y * geometry.scale,
            right: 0.0,
            bottom: 0.0,
        });
        iced::widget::stack![layered, positioned].into()
    } else {
        layered.into()
    };
```

Expose the regular annotation font from `canvas.rs` for the editor:

```rust
pub(crate) fn annotation_font() -> iced::Font {
    ANNOTATION_FONT
}
```

(`Enter` stays the default line-break binding — spec §9.3 multi-line notes; `Esc` is not a default text_editor binding, so it bubbles to the global subscription and cancels via `EscapePressed`.)

- [ ] **Step 5: Run tests**

Run: `rtk cargo test -p rollshot-app`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
rtk git add crates/rollshot-app
rtk git commit -m "feat(app): inline multi-line text notes with single-edit commits"
```

---

### Task 21: Navigator drawer

**Files:**
- Create: `crates/rollshot-app/src/result_workspace/navigator.rs`
- Modify: `crates/rollshot-app/src/result_workspace/mod.rs` (`mod navigator;`)
- Modify: `crates/rollshot-app/src/result_workspace/canvas.rs` (Navigator cache on `EditorState`)
- Modify: `crates/rollshot-app/src/result_workspace/update.rs` (jump message/arm, cache refresh)
- Modify: `crates/rollshot-app/src/result_workspace/view.rs` (drawer in the layout row)

Spec §13 requires Navigator order to be recomputed **only when annotation geometry or membership changes** — never per frame. The order is therefore cached on `EditorState`, keyed by the document `state_id` (which changes exactly when the annotation graph changes), and refreshed once per `update()` cycle.

- [ ] **Step 1: Write failing tests** (in `navigator.rs`)

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::result_workspace::viewport::{geometry_for, ZoomMode};
    use iced::{Size, Vector};
    use rollshot_image_document::ImagePoint;

    #[test]
    fn jump_centers_the_target_and_clamps_to_scroll_range() {
        let geometry = geometry_for(
            ZoomMode::ActualSize,
            Size::new(1000.0, 5000.0),
            Size::new(500.0, 400.0),
        );
        // Target deep in the image: centered → offset = target − viewport/2.
        let offset = jump_offset(ImagePoint::new(500.0, 2000.0), &geometry, Size::new(500.0, 400.0));
        assert_eq!(Vector::new(offset.x, offset.y), Vector::new(250.0, 1800.0));
        // Target near the top: clamps to zero.
        let top = jump_offset(ImagePoint::new(10.0, 10.0), &geometry, Size::new(500.0, 400.0));
        assert_eq!(Vector::new(top.x, top.y), Vector::new(0.0, 0.0));
    }
}
```

And in `update.rs` tests:

```rust
    #[test]
    fn navigator_jump_selects_and_ignores_stale_ids() {
        let mut state = workspace_with_size(100, 100);
        let id = state.document.image.add_number_callout(
            ImagePoint::new(10.0, 10.0),
            ImagePoint::new(10.0, 10.0),
        );
        let _ = update(&mut state, Message::NavigatorJump(id));
        assert_eq!(state.editor.selection, Some(id));
        // Stale id (after undo): ignored, selection cleared (spec §15).
        let _ = update(&mut state, Message::Undo);
        let _ = update(&mut state, Message::NavigatorJump(id));
        assert_eq!(state.editor.selection, None);
    }
```

- [ ] **Step 2: Run to verify failure**

Run: `rtk cargo test -p rollshot-app jump_`
Expected: FAIL.

- [ ] **Step 3: Implement `navigator.rs`**

```rust
//! Navigator drawer (spec §8.2): semantic top-to-bottom annotation list with
//! jump-to-annotation. Ordering comes from the document crate; this module
//! owns only the view and the viewport jump math.

use iced::widget::{button, column, container, scrollable, text};
use iced::{Alignment, Element, Length, Size};
use rollshot_image_document::{ImagePoint, NavigatorItem};

use super::update::Message;
use super::viewport::{clamp_scroll, ViewportGeometry};
use super::ResultWorkspace;

pub(crate) const NAVIGATOR_WIDTH: f32 = 220.0;

/// Absolute scroll offset that centers `target` (image coords) in the
/// viewport, clamped to the scrollable range.
pub(crate) fn jump_offset(
    target: ImagePoint,
    geometry: &ViewportGeometry,
    viewport: Size,
) -> scrollable::AbsoluteOffset {
    let content_x = geometry.image_origin.x + target.x * geometry.scale;
    let content_y = geometry.image_origin.y + target.y * geometry.scale;
    let clamped = clamp_scroll(
        iced::Vector::new(content_x - viewport.width / 2.0, content_y - viewport.height / 2.0),
        geometry.max_scroll,
    );
    scrollable::AbsoluteOffset { x: clamped.x, y: clamped.y }
}

pub(crate) fn navigator_panel(state: &ResultWorkspace) -> Element<'_, Message> {
    // Reads the cached order (spec §13) — never recomputes in the view.
    let items = &state.editor.navigator_items;
    let mut list = column![].spacing(2);
    if items.is_empty() {
        list = list.push(text("No annotations yet").size(13));
    }
    for item in items {
        let selected = state.editor.selection == Some(item.id);
        let row_btn = button(text(item.label.clone()).size(13))
            .width(Length::Fill)
            .style(if selected { button::primary } else { button::text })
            .on_press(Message::NavigatorJump(item.id));
        list = list.push(row_btn);
    }
    container(scrollable(list).height(Length::Fill))
        .width(Length::Fixed(NAVIGATOR_WIDTH))
        .padding(6)
        .align_x(Alignment::Start)
        .into()
}
```

Add the cache to `EditorState` in `canvas.rs` (two fields plus initializers in `EditorState::new`, both defaulting to empty/`None`):

```rust
    /// Cached Navigator order, refreshed only when the document changes
    /// (spec §13). Keyed by the document state_id.
    pub navigator_items: Vec<rollshot_image_document::NavigatorItem>,
    pub navigator_items_state: Option<u64>,
```

And in `update.rs`, rename the existing `update` to `update_inner` and wrap it so every message cycle ends with a cheap staleness check:

```rust
pub(crate) fn update(state: &mut ResultWorkspace, message: Message) -> Task<Message> {
    let task = update_inner(state, message);
    refresh_navigator(state);
    task
}

/// Recompute the Navigator order only when the annotation graph changed
/// (spec §13). `state_id` changes exactly on commit/undo/redo.
fn refresh_navigator(state: &mut ResultWorkspace) {
    let current = state.document.image.state_id();
    if state.editor.navigator_items_state != Some(current) {
        state.editor.navigator_items = state.document.image.navigator_items();
        state.editor.navigator_items_state = Some(current);
    }
}
```

Tests that mutate the document directly (bypassing `update`) and then assert on Navigator content must send any message (or call `refresh_navigator`) first — message-driven tests get this for free.

- [ ] **Step 4: Wire message and view**

`update.rs` — add `NavigatorJump(rollshot_image_document::AnnotationId)` and arm:

```rust
        Message::NavigatorJump(id) => {
            commit_text_draft(state);
            if state.document.image.annotation(id).is_none() {
                state.editor.selection = None; // stale item (spec §15)
                return Task::none();
            }
            state.editor.selection = Some(id);
            // The cache is current here: the annotation exists (guard above)
            // and refresh_navigator ran at the end of the previous cycle.
            let Some(target) = state
                .editor
                .navigator_items
                .iter()
                .find(|i| i.id == id)
                .map(|i| i.center)
            else {
                return Task::none();
            };
            let geometry = geometry_for(
                state.viewport.zoom,
                state_original_size(state),
                state.viewport_bounds,
            );
            iced::widget::operation::scroll_to(
                state.scrollable_id.clone(),
                super::navigator::jump_offset(target, &geometry, state.viewport_bounds),
            )
        }
```

`view.rs` — wrap the canvas area in a row with the drawer:

```rust
    let canvas_area = canvas_view(state, original);
    let workspace_row: Element<'_, Message> = if state.editor.navigator_open {
        row![canvas_area, super::navigator::navigator_panel(state)]
            .spacing(4)
            .into()
    } else {
        canvas_area
    };
```

and use `workspace_row` in the column layout where `canvas` was.

(Selection sync canvas → Navigator is automatic: the panel highlights `state.editor.selection`, which canvas presses set in Task 19.)

- [ ] **Step 5: Run tests**

Run: `rtk cargo test -p rollshot-app`
Expected: PASS — including `navigator_defaults_open_for_tall_images_only` from Task 14.

- [ ] **Step 6: Commit**

```bash
rtk git add crates/rollshot-app
rtk git commit -m "feat(app): navigator drawer with centered jumps and selection sync"
```

---

### Task 22: Keyboard routing — shortcuts, Esc, capture gating

**Files:**
- Modify: `crates/rollshot-app/src/result_workspace/update.rs`

- [ ] **Step 1: Write failing tests** (in `update.rs` tests)

```rust
    fn zmod() -> keyboard::Modifiers {
        #[cfg(target_os = "macos")]
        { keyboard::Modifiers::COMMAND }
        #[cfg(not(target_os = "macos"))]
        { keyboard::Modifiers::CTRL }
    }

    #[test]
    fn key_mapping_routes_tools_undo_redo_delete_copy() {
        use keyboard::{key::Named, Key};
        let none = keyboard::Modifiers::default();
        assert!(matches!(
            map_key_press(&Key::Character("n".into()), none, false),
            Some(Message::SelectTool(Tool::Number))
        ));
        assert!(matches!(
            map_key_press(&Key::Character("z".into()), zmod(), false),
            Some(Message::Undo)
        ));
        assert!(matches!(
            map_key_press(&Key::Character("z".into()), zmod() | keyboard::Modifiers::SHIFT, false),
            Some(Message::Redo)
        ));
        assert!(matches!(
            map_key_press(&Key::Named(Named::Delete), none, false),
            Some(Message::DeleteSelected)
        ));
        assert!(matches!(
            map_key_press(&Key::Character("c".into()), zmod(), false),
            Some(Message::Copy)
        ));
    }

    #[test]
    fn captured_keys_are_ignored_except_escape() {
        use keyboard::{key::Named, Key};
        let none = keyboard::Modifiers::default();
        // While the text editor has focus it captures these — no shortcut fires.
        assert!(map_key_press(&Key::Character("n".into()), none, true).is_none());
        assert!(map_key_press(&Key::Named(Named::Backspace), none, true).is_none());
        // Escape always routes (the update arm owns the priority).
        assert!(matches!(
            map_key_press(&Key::Named(Named::Escape), none, true),
            Some(Message::EscapePressed)
        ));
    }

    #[test]
    fn plain_characters_do_not_fire_with_command_modifiers_held() {
        use keyboard::Key;
        assert!(map_key_press(&Key::Character("n".into()), zmod(), false).is_none());
    }
```

- [ ] **Step 2: Run to verify failure**

Run: `rtk cargo test -p rollshot-app key_mapping`
Expected: FAIL.

- [ ] **Step 3: Implement and rewire the subscription** (`update.rs`)

```rust
/// Map a key press to a workspace message. `captured` is true when a widget
/// (the inline text editor) already consumed the event — only Esc routes
/// then, so typing never triggers tools/delete (spec §9.5 gating).
pub(crate) fn map_key_press(
    key: &keyboard::Key,
    modifiers: keyboard::Modifiers,
    captured: bool,
) -> Option<Message> {
    use keyboard::key::Named;
    if matches!(key, keyboard::Key::Named(Named::Escape)) {
        return Some(Message::EscapePressed);
    }
    if captured {
        return None;
    }
    let command = zoom_modifier_held(modifiers); // Cmd on macOS, Ctrl elsewhere
    match key {
        keyboard::Key::Named(Named::Delete) | keyboard::Key::Named(Named::Backspace) => {
            Some(Message::DeleteSelected)
        }
        keyboard::Key::Character(c) if command => match c.as_str() {
            "z" if modifiers.shift() => Some(Message::Redo),
            "z" => Some(Message::Undo),
            "c" => Some(Message::Copy),
            _ => None,
        },
        keyboard::Key::Character(c) if !modifiers.alt() => match c.as_str() {
            "v" => Some(Message::SelectTool(canvas::Tool::Select)),
            "n" => Some(Message::SelectTool(canvas::Tool::Number)),
            "t" => Some(Message::SelectTool(canvas::Tool::Text)),
            "r" => Some(Message::SelectTool(canvas::Tool::Redact)),
            _ => None,
        },
        _ => None,
    }
}
```

Rewire `subscription`'s `listen_with` (replacing the old Escape→RequestClose arm):

```rust
        iced::event::listen_with(|event, status, _window| match event {
            iced::Event::Keyboard(keyboard::Event::ModifiersChanged(m)) => {
                Some(Message::ModifiersChanged(m))
            }
            iced::Event::Keyboard(keyboard::Event::KeyPressed { key, modifiers, .. }) => {
                map_key_press(&key, modifiers, status == iced::event::Status::Captured)
            }
            _ => None,
        }),
```

Note `zoom_modifier_held` already encodes the platform command modifier; reuse it rather than duplicating the cfg.

Update the pre-existing test `operating_system_close_uses_unsaved_close_confirmation` if it asserted the old Escape mapping; window-manager close (`close_requests`) still maps to `RequestClose` unchanged.

- [ ] **Step 4: Run tests**

Run: `rtk cargo test -p rollshot-app`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
rtk git add crates/rollshot-app
rtk git commit -m "feat(app): keyboard shortcuts with text-editor capture gating"
```

---

### Task 23: Full verification

- [ ] **Step 1: Full test suite, formatting, lints**

```bash
rtk cargo test --workspace
rtk cargo fmt --check
rtk cargo clippy --workspace --all-targets -- -D warnings
rtk git diff --check
```

Expected: all clean. (No `rollshot-core` stitching paths were touched, so the bench workflow from AGENTS.md §7 does not apply.)

- [ ] **Step 2: Linux runtime verification** (manual, spec §16.3)

Build and run a capture → Result Workspace session (`rtk cargo run -p rollshot-app`, or open via the normal capture flow) and verify:

1. Toolbar: icon tools show active state; Undo/Redo disable correctly; Copy/Save As/Reveal are labeled; tooltips show shortcuts.
2. Number: click stamps 1, 2, 3…; drag separates tip/bubble; select + drag handles move tip and bubble independently; delete a middle callout → remaining renumber compactly.
3. Text: click opens inline editor at the point; Enter makes a newline; Ctrl+Enter and click-outside commit; Esc cancels; double-click re-edits; CJK text renders in both the live note and a Save As export.
4. Redact: drag draws an opaque rectangle; resize via 8 handles; export shows solid pixels.
5. Select: drag empty canvas pans; wheel zoom/pan still work during and after tool use; entering a tool never resets zoom/scroll; scrollbar drags don't create annotations.
6. Navigator: defaults open on a tall capture, closed on a short one; clicking an item scrolls the annotation to center and selects it; canvas selection highlights the item.
7. Copy ▾: Copy puts the annotated image on the clipboard; Copy Original pastes the unredacted source; menu closes on outside click.
8. Save As: writes annotated output; Reveal opens the export; title and dirty state behave; close prompts distinguish capture vs edits.
9. Long capture (>8000 px tall): pan/zoom responsive with ~100 annotations; flatten completes on Copy/Save without UI lockup beyond a brief pause.

- [ ] **Step 3: macOS runtime verification** (same checklist, plus)

- `rtk cargo check -p rollshot-app` ON MACOS — confirms `macos_product.rs` compiles against the refactor (Linux CI cannot).
- Cmd-based shortcuts and Cmd+Enter commit.
- Daemon font loading: number labels and notes render in DejaVu Sans.

If no macOS machine is available this session, state that explicitly in the final report as the remaining runtime-verification risk (per AGENTS.md platform-split rules; the Result Workspace is shared active iced code, so logic parity is by construction — the risk is compile/runtime only).

- [ ] **Step 4: Final commit (if fixups were needed)**

```bash
rtk git add -A && rtk git commit -m "test(app): runtime verification fixups for long-shot callouts"
```

---

## Spec coverage map (self-review)

| Spec section | Tasks |
|---|---|
| §5 crate boundary + README | 1 |
| §6 document model, invariants, D1 renumbering, visual defaults | 2, 3, 5, 6, 7 |
| §7 workspace state, source/export paths | 14, 16 |
| §8.1 toolbar (D2) | 17 |
| §8.2 Navigator + threshold | 10, 12, 14, 21 |
| §9.1–9.4 tools | 18, 19, 20 |
| §9.5 Esc priority | 16 (+20 draft cancel) |
| §10 undo/redo | 6, 16 |
| §11.1 live rendering + culling | 18 |
| §11.2 flattened output, parity | 8, 11 (shared shapes) |
| §12.1 Copy / Copy Original, dirty untouched | 15, 17 |
| §12.2 Save As semantics | 14, 15 |
| §12.3 close confirmation | 14 |
| §13 long-image behavior/perf | 11 (scale test), 18 (culling), 23 (runtime) |
| §14 module refactor | 13 |
| §15 error handling | 5, 7 (invariant rejection), 16/19/21 (stale selection, inline errors), 14 (failed save) |
| §16 testing + commands | per-task tests, 23 |
| §17 constraints (no Tauri changes, no speculative APIs) | global — only `macos_product.rs` font lines outside result_workspace |

## Notes for the implementer

- **Bubble drag intent:** pressing a bubble starts a `NumberBubble` drag (bubble moves alone); the `Body` part for callouts is unreachable from hit-testing by design — `dragged_annotation` still defines it for totality.
- **`iced::widget::Id` vs scrollable id:** both `scrollable_id` and `text_editor_id` are `iced::widget::Id::unique()` stored on the workspace; never recreate per-view.
- **Test pattern:** all gesture/update tests run through `update()` with `workspace_with_size` at `ActualSize` zoom so image coordinates equal screen coordinates (tolerance math stays honest).
- **Do not** touch `crates/rollshot-tauri-app` or capture overlay crates anywhere in this plan (spec §17).
- Commit after every task; never commit on red tests.
