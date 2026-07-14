# Freehand Tools (Slice 4) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add complete Pen and Highlighter freehand annotation lifecycles (create → preview → select → move → style-edit → delete → undo/redo → Navigator → flatten) to the Result Workspace.

**Architecture:** A new `Annotation::Freehand` variant stores a simplified polyline in `rollshot-image-document`; a new `RenderShape::Polyline` primitive renders it with round caps/joins and whole-stroke uniform-alpha compositing in both the raster flattener and the iced canvas. `rollshot-app` owns pointer sampling (2-screen-px distance filter), commit-time RDP simplification (1-screen-px epsilon), tool/toolbar/defaults/properties wiring, and the editor's first opacity control (Highlighter only).

**Tech Stack:** Rust workspace; `rollshot-image-document` (pure, no UI deps), `rollshot-app` (iced 0.14), `toml` config, `tracing` diagnostics.

**Authority:** Spec [`docs/superpowers/specs/2026-07-14-freehand-tools-design.md`](../specs/2026-07-14-freehand-tools-design.md) under umbrella [`2026-07-12-annotation-editor-umbrella-design.md`](../specs/2026-07-12-annotation-editor-umbrella-design.md). On conflict, the spec wins over this plan.

## Global Constraints

- Prefix every shell command with `rtk` (e.g. `rtk cargo test -p rollshot-image-document`).
- Pen canonical default: color `#E5484D`, width `4.0`, opacity `1.0` (this is `StrokeStyle::default()`).
- Highlighter canonical default: color `#FFD400`, width `12.0`, opacity `0.4`.
- Sampling filter: 2 screen px; RDP epsilon: 1 screen px; minimum gesture: 4 screen px on the path bounding box's larger dimension. All screen-space values divide by the viewport scale.
- Uniform per-stroke alpha: one stroke blends exactly once per pixel (max coverage across segments, never summed). Separate strokes composite source-over in document order.
- Opacity is exposed ONLY for Highlighter targets (tool defaults and selected Highlighter annotations). Persisted opacity round-trips only for the `highlighter` config key; all other stroke keys keep force-to-1.0.
- No freehand resize handles, no per-point editing, no Shift constraints. Body move, delete, and style edit only.
- Shortcuts: `P` = Pen, `H` = Highlighter. Narrow toolbar: Pen stays visible; Highlighter routes into More (with Line and Redact).
- Diagnostics use `tracing` with target `rollshot::annotation` and structured fields; no `println!`/`dbg!`.
- Tasks touching `crates/rollshot-app/src/result_workspace/canvas.rs` or `toolbar.rs` iced code MUST invoke the `iced-rs` skill first (workspace pins iced 0.14).
- Commit messages use conventional commits and end with `Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>`.
- Existing test suites must keep passing after every task. Known pre-existing failures (3 stale-`config.toml` failures in `result_workspace::tests`) are not regressions.

---

### Task 1: Freehand geometry helpers in the document crate

**Files:**
- Create: `crates/rollshot-image-document/src/freehand.rs`
- Modify: `crates/rollshot-image-document/src/lib.rs` (add `mod freehand;` alongside the existing `mod two_point;` / `mod box_shape;` lines — match the existing module list style; do not re-export publicly)

**Interfaces:**
- Produces: `pub(crate) fn polyline_distance(point: ImagePoint, points: &[ImagePoint]) -> f32` — minimum distance from `point` to the polyline (per-segment clamped projection; zero-length segments fall back to point distance).
- Produces: `pub(crate) fn freehand_bounds(points: &[ImagePoint], width: f32) -> ImageRect` — AABB of the points expanded by `width / 2.0`.
- Consumes: `crate::two_point::segment_distance`, `crate::geometry::{ImagePoint, ImageRect}`.

- [ ] **Step 1: Write the failing tests**

Create `crates/rollshot-image-document/src/freehand.rs`:

```rust
//! Freehand polyline geometry: bounds and distance used by hit testing and
//! culling (Slice 4 spec §6.4/§6.5).

use crate::geometry::{ImagePoint, ImageRect};
use crate::two_point::segment_distance;

/// Minimum distance from `point` to any segment of the polyline. Consecutive
/// duplicate points contribute a point-distance (no zero-length segment math).
/// A single-point slice degenerates to point distance.
pub(crate) fn polyline_distance(point: ImagePoint, points: &[ImagePoint]) -> f32 {
    debug_assert!(!points.is_empty());
    let mut best = f32::MAX;
    for pair in points.windows(2) {
        let d = if pair[0] == pair[1] {
            point.distance(pair[0])
        } else {
            segment_distance(point, pair[0], pair[1])
        };
        best = best.min(d);
    }
    if points.len() == 1 {
        best = point.distance(points[0]);
    }
    best
}

/// Conservative visual bounds: AABB of the points expanded by half the
/// stroke width (round caps extend half a width past the endpoints).
pub(crate) fn freehand_bounds(points: &[ImagePoint], width: f32) -> ImageRect {
    debug_assert!(!points.is_empty());
    let mut x0 = f32::MAX;
    let mut y0 = f32::MAX;
    let mut x1 = f32::MIN;
    let mut y1 = f32::MIN;
    for p in points {
        x0 = x0.min(p.x);
        y0 = y0.min(p.y);
        x1 = x1.max(p.x);
        y1 = y1.max(p.y);
    }
    ImageRect {
        x: x0,
        y: y0,
        width: x1 - x0,
        height: y1 - y0,
    }
    .expanded(width / 2.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn l_path() -> Vec<ImagePoint> {
        vec![
            ImagePoint::new(0.0, 0.0),
            ImagePoint::new(10.0, 0.0),
            ImagePoint::new(10.0, 10.0),
        ]
    }

    #[test]
    fn distance_on_segment_is_zero() {
        assert_eq!(polyline_distance(ImagePoint::new(5.0, 0.0), &l_path()), 0.0);
    }

    #[test]
    fn distance_uses_nearest_segment() {
        // Point near the vertical leg, far from the horizontal leg.
        let d = polyline_distance(ImagePoint::new(13.0, 8.0), &l_path());
        assert!((d - 3.0).abs() < 1e-4);
    }

    #[test]
    fn distance_in_empty_corner_is_not_zero() {
        // Inside the AABB but far from both legs (the bounding-box-only trap):
        // (2, 8) is 8.0 from the horizontal leg (projects to (2, 0)) and 8.0
        // from the vertical leg (projects to (10, 8)).
        let d = polyline_distance(ImagePoint::new(2.0, 8.0), &l_path());
        assert!((d - 8.0).abs() < 1e-4);
    }

    #[test]
    fn duplicate_consecutive_points_do_not_panic() {
        let pts = vec![
            ImagePoint::new(0.0, 0.0),
            ImagePoint::new(0.0, 0.0),
            ImagePoint::new(4.0, 0.0),
        ];
        assert_eq!(polyline_distance(ImagePoint::new(2.0, 0.0), &pts), 0.0);
    }

    #[test]
    fn bounds_expand_by_half_width() {
        let b = freehand_bounds(&l_path(), 4.0);
        assert_eq!(
            b,
            ImageRect {
                x: -2.0,
                y: -2.0,
                width: 14.0,
                height: 14.0
            }
        );
    }
}
```

- [ ] **Step 2: Register the module and run the tests to verify they fail, then pass**

Add `mod freehand;` to `crates/rollshot-image-document/src/lib.rs` next to `mod two_point;`.

Run: `rtk cargo test -p rollshot-image-document freehand`
Expected: the 5 new tests PASS (the module is written with its tests; if any fail, fix the helper, not the test).

Note: `two_point::segment_distance` is `pub(crate)`-reachable — check its visibility in `crates/rollshot-image-document/src/two_point.rs:28`; it is `pub fn` in a private module, so `crate::two_point::segment_distance` resolves.

- [ ] **Step 3: Commit**

```bash
rtk git add crates/rollshot-image-document/src/freehand.rs crates/rollshot-image-document/src/lib.rs
rtk git commit -m "feat(annotation): add freehand polyline geometry helpers"
```

---

### Task 2: `RenderShape::Polyline` and the uniform-alpha raster stroke

**Files:**
- Modify: `crates/rollshot-image-document/src/shapes.rs` (add variant, ~line 21-58)
- Modify: `crates/rollshot-image-document/src/raster.rs` (add `stroke_polyline`)
- Modify: `crates/rollshot-image-document/src/flatten.rs` (new `draw_shape` arm, ~line 26)
- Modify: `crates/rollshot-app/src/result_workspace/canvas.rs` (new `draw_shape` arm, ~line 322 — the `RenderShape` match is exhaustive and will not compile without it)

**Interfaces:**
- Produces: `RenderShape::Polyline { points: Vec<ImagePoint>, width: f32, color: Rgba8 }` — round caps and round joins; `color.a` is the whole-stroke uniform alpha.
- Produces: `pub(crate) fn stroke_polyline(img: &mut RgbaImage, points: &[ImagePoint], width: f32, color: Rgba8)` in `raster.rs`.
- Consumes: `freehand::polyline_distance` (Task 1), `raster::blend_px`.

**Before iced work in this task, invoke the `iced-rs` skill.**

- [ ] **Step 1: Write failing raster tests**

Append to the `#[cfg(test)] mod tests` in `crates/rollshot-image-document/src/flatten.rs` (mirror the existing `line_opacity_blends_once_at_full_coverage` test at `flatten.rs:106` for helpers like image construction — reuse its patterns for creating a solid RgbaImage):

```rust
#[test]
fn polyline_self_crossing_blends_alpha_exactly_once() {
    // A figure-X stroke crossing itself at the center with 50% alpha over
    // white: the crossing pixel must equal one blend, not two.
    let mut img = RgbaImage::from_pixel(40, 40, image::Rgba([255, 255, 255, 255]));
    let points = vec![
        ImagePoint::new(5.0, 5.0),
        ImagePoint::new(35.0, 35.0),
        ImagePoint::new(5.0, 35.0),
        ImagePoint::new(35.0, 5.0),
    ];
    let color = crate::geometry::Rgba8::new(0, 0, 0, 128);
    crate::raster::stroke_polyline(&mut img, &points, 4.0, color);
    // One source-over blend of a=128/255 over 255 → ~127.
    let crossing = img.get_pixel(20, 20).0[0];
    assert!((126..=129).contains(&crossing), "got {crossing}");
    // A point on only one leg blends identically (uniformity).
    let single = img.get_pixel(10, 10).0[0];
    assert_eq!(crossing, single);
}

#[test]
fn two_separate_polylines_darken_where_they_cross() {
    let mut img = RgbaImage::from_pixel(40, 40, image::Rgba([255, 255, 255, 255]));
    let color = crate::geometry::Rgba8::new(0, 0, 0, 128);
    let a = vec![ImagePoint::new(5.0, 20.0), ImagePoint::new(35.0, 20.0)];
    let b = vec![ImagePoint::new(20.0, 5.0), ImagePoint::new(20.0, 35.0)];
    crate::raster::stroke_polyline(&mut img, &a, 4.0, color);
    crate::raster::stroke_polyline(&mut img, &b, 4.0, color);
    let crossing = img.get_pixel(20, 20).0[0];
    let single = img.get_pixel(10, 20).0[0];
    assert!(crossing < single, "two strokes must darken: {crossing} vs {single}");
}

#[test]
fn polyline_has_round_caps() {
    // A pixel half a stroke-width beyond the endpoint, along the direction of
    // travel, is covered by the round cap.
    let mut img = RgbaImage::from_pixel(40, 40, image::Rgba([255, 255, 255, 255]));
    let points = vec![ImagePoint::new(10.0, 20.0), ImagePoint::new(30.0, 20.0)];
    crate::raster::stroke_polyline(
        &mut img,
        &points,
        8.0,
        crate::geometry::Rgba8::new(0, 0, 0, 255),
    );
    // (33, 20) is 3px past the endpoint, inside the radius-4 cap.
    assert!(img.get_pixel(33, 20).0[0] < 128);
    // (36, 20) is 6px past — outside the cap.
    assert_eq!(img.get_pixel(36, 20).0[0], 255);
}
```

Run: `rtk cargo test -p rollshot-image-document polyline`
Expected: FAIL — `stroke_polyline` not found.

- [ ] **Step 2: Implement `stroke_polyline` in `raster.rs`**

Add after `stroke_line` (~line 125). The whole-stroke uniform alpha comes from computing per-pixel MIN distance to the whole polyline (which gives MAX coverage), then blending exactly once. Clamped-projection distance produces round caps and joins for free:

```rust
/// Anti-aliased polyline stroke with round caps and joins. The whole stroke
/// composites with ONE source-over blend per pixel (coverage from the minimum
/// distance to any segment), so a self-crossing stroke never darkens at its
/// own overlaps (Slice 4 spec §8.2).
pub(crate) fn stroke_polyline(
    img: &mut RgbaImage,
    points: &[ImagePoint],
    width: f32,
    color: Rgba8,
) {
    if img.width() == 0 || img.height() == 0 || points.len() < 2 {
        return;
    }
    let radius = width / 2.0;
    let bounds = crate::freehand::freehand_bounds(points, width).expanded(1.0);
    let max_x = i32::try_from(img.width() - 1).unwrap_or(i32::MAX);
    let max_y = i32::try_from(img.height() - 1).unwrap_or(i32::MAX);
    let x0 = (bounds.x.floor() as i32).max(0);
    let y0 = (bounds.y.floor() as i32).max(0);
    let x1 = ((bounds.x + bounds.width).ceil() as i32).min(max_x);
    let y1 = ((bounds.y + bounds.height).ceil() as i32).min(max_y);
    for y in y0..=y1 {
        for x in x0..=x1 {
            let p = ImagePoint::new(x as f32 + 0.5, y as f32 + 0.5);
            let d = crate::freehand::polyline_distance(p, points);
            let coverage = (radius + 0.5 - d).clamp(0.0, 1.0);
            if coverage > 0.0 {
                blend_px(img, x, y, color, coverage);
            }
        }
    }
}
```

Performance note: this is O(pixels-in-bounds × segments) and runs only at explicit flatten. If the 100-annotation long-image test (Task 10) becomes slow (> a few seconds), add a per-row segment prefilter (skip segments whose own AABB expanded by `radius + 1.0` misses the pixel) — do not change the compositing semantics.

- [ ] **Step 3: Add the `RenderShape::Polyline` variant and the flatten arm**

In `crates/rollshot-image-document/src/shapes.rs`, add to the `RenderShape` enum after `Line`:

```rust
    Polyline {
        points: Vec<ImagePoint>,
        width: f32,
        color: Rgba8,
    },
```

In `crates/rollshot-image-document/src/flatten.rs`, add to `draw_shape` after the `Line` arm (import `stroke_polyline` in the existing `use crate::raster::{...}` list):

```rust
        RenderShape::Polyline {
            points,
            width,
            color,
        } => stroke_polyline(img, points, *width, *color),
```

- [ ] **Step 4: Add the iced canvas arm (invoke `iced-rs` skill first)**

In `crates/rollshot-app/src/result_workspace/canvas.rs`, `AnnotationCanvas::draw_shape` (~line 322), add after the `Line` arm:

```rust
            RenderShape::Polyline {
                points,
                width,
                color,
            } => {
                if points.len() >= 2 {
                    let path = canvas::Path::new(|b| {
                        b.move_to(Point::new(points[0].x * s, points[0].y * s));
                        for p in &points[1..] {
                            b.line_to(Point::new(p.x * s, p.y * s));
                        }
                    });
                    frame.stroke(
                        &path,
                        canvas::Stroke {
                            line_cap: canvas::LineCap::Round,
                            line_join: canvas::LineJoin::Round,
                            ..canvas::Stroke::default()
                        }
                        .with_color(token_color(*color))
                        .with_width(width * s),
                    );
                }
            }
```

Verify the iced 0.14 `canvas::Stroke` field/builder names against the `iced-rs` skill reference before compiling; adjust to the actual 0.14 API if `line_cap`/`line_join` differ.

Known, accepted deviation (spec §8.3): lyon stroke tessellation may double-blend translucent fragments at self-overlaps in the LIVE preview only. Flatten is authoritative. If runtime verification later finds this unacceptable, escalate via `rollshot-run-spike` — do not silently change the flatten semantics.

- [ ] **Step 5: Run tests and workspace build**

Run: `rtk cargo test -p rollshot-image-document polyline`
Expected: 3 new tests PASS.
Run: `rtk cargo test`
Expected: full suite passes (plus the 3 known pre-existing failures).

- [ ] **Step 6: Commit**

```bash
rtk git add crates/rollshot-image-document/src/shapes.rs crates/rollshot-image-document/src/raster.rs crates/rollshot-image-document/src/flatten.rs crates/rollshot-app/src/result_workspace/canvas.rs
rtk git commit -m "feat(annotation): add Polyline render command with uniform-alpha raster stroke"
```

---

### Task 3: `Annotation::Freehand` document model, edit ops, and all consumer arms

This is the largest task because adding an enum variant forces every exhaustive match in the workspace to gain an arm in the same commit.

**Files:**
- Modify: `crates/rollshot-image-document/src/annotation.rs` (variant, `FreehandKind`, constructors, `id`/`anchor`/`stroke_style` arms)
- Modify: `crates/rollshot-image-document/src/style.rs` (`StrokeStyle::highlighter_default`)
- Modify: `crates/rollshot-image-document/src/edit_op.rs` (`AddFreehand`, `UpdateFreehandPoints`)
- Modify: `crates/rollshot-image-document/src/document.rs` (validation, `apply_batch` referenced-id match, `apply_one` arms, wrappers, `EditError::InvalidFreehandPath`, extend `UpdateStrokeStyle` arm)
- Modify: `crates/rollshot-image-document/src/shapes.rs` (`annotation_shapes` + `annotation_bounds` arms)
- Modify: `crates/rollshot-image-document/src/hit.rs` (hit arm)
- Modify: `crates/rollshot-image-document/src/navigator.rs` (labels)
- Modify: `crates/rollshot-image-document/src/lib.rs` (re-export `FreehandKind` next to `TwoPointKind`/`ShapeKind`)
- Modify (compile-required app arms): `crates/rollshot-app/src/result_workspace/canvas.rs` (`draw_selection_handles`), `crates/rollshot-app/src/result_workspace/properties.rs` (`preview_annotation`), `crates/rollshot-app/src/result_workspace/update.rs` (`handle_canvas_released` `EditAnnotation` commit match)
- Check-and-fix: run `rtk cargo build --workspace` and add minimal arms to ANY other exhaustive `Annotation` match the compiler reports (automation lowering, workbench, Timeline, Action Guide consumers). Non-Result-Workspace consumers get display/passthrough behavior only — never freehand creation.

**Interfaces:**
- Produces (doc crate public API):
  - `pub enum FreehandKind { Pen, Highlighter }` (Copy, Eq, serde like `ShapeKind`)
  - `Annotation::Freehand { id, kind, points: Vec<ImagePoint>, style: StrokeStyle }`
  - `Annotation::freehand(id, kind, points)` / `Annotation::freehand_with_style(id, kind, points, style)`
  - `StrokeStyle::highlighter_default() -> StrokeStyle` (`#FFD400`, `12.0`, `0.4`)
  - `EditOp::AddFreehand { kind, points, style }`, `EditOp::UpdateFreehandPoints { id, points }`
  - `ImageDocument::add_freehand_with_style(kind, points, style) -> Result<AnnotationId, EditError>`
  - `ImageDocument::set_freehand_points(id, points) -> Result<(), EditError>`
  - `EditError::InvalidFreehandPath`
- Consumes: Task 1 helpers, Task 2 `RenderShape::Polyline`.

- [ ] **Step 1: Write failing document tests**

Append to `crates/rollshot-image-document/src/document.rs` tests (mirror the existing add/update/undo tests at `document.rs:867+`):

```rust
#[test]
fn freehand_add_validates_and_commits_one_entry() {
    let mut doc = ImageDocument::new(RgbaImage::new(100, 100));
    let pts = vec![
        ImagePoint::new(10.0, 10.0),
        ImagePoint::new(50.0, 20.0),
        ImagePoint::new(60.0, 70.0),
    ];
    let id = doc
        .add_freehand_with_style(
            crate::FreehandKind::Pen,
            pts.clone(),
            StrokeStyle::default(),
        )
        .unwrap();
    assert!(matches!(
        doc.annotation(id),
        Some(Annotation::Freehand { kind: crate::FreehandKind::Pen, points, .. })
            if *points == pts
    ));
    assert!(doc.undo());
    assert!(doc.annotation(id).is_none());
}

#[test]
fn freehand_rejects_degenerate_paths() {
    let mut doc = ImageDocument::new(RgbaImage::new(100, 100));
    let style = StrokeStyle::default();
    // Fewer than two points.
    assert_eq!(
        doc.add_freehand_with_style(
            crate::FreehandKind::Pen,
            vec![ImagePoint::new(1.0, 1.0)],
            style
        ),
        Err(EditError::InvalidFreehandPath)
    );
    // No two distinct points.
    assert_eq!(
        doc.add_freehand_with_style(
            crate::FreehandKind::Pen,
            vec![ImagePoint::new(1.0, 1.0), ImagePoint::new(1.0, 1.0)],
            style
        ),
        Err(EditError::InvalidFreehandPath)
    );
    // Non-finite point.
    assert_eq!(
        doc.add_freehand_with_style(
            crate::FreehandKind::Pen,
            vec![ImagePoint::new(f32::NAN, 1.0), ImagePoint::new(2.0, 2.0)],
            style
        ),
        Err(EditError::NonFiniteCoordinate)
    );
    assert!(!doc.can_undo());
}

#[test]
fn freehand_points_update_preserves_id_kind_style() {
    let mut doc = ImageDocument::new(RgbaImage::new(100, 100));
    let style = StrokeStyle::highlighter_default();
    let id = doc
        .add_freehand_with_style(
            crate::FreehandKind::Highlighter,
            vec![ImagePoint::new(0.0, 0.0), ImagePoint::new(10.0, 0.0)],
            style,
        )
        .unwrap();
    let moved = vec![ImagePoint::new(5.0, 5.0), ImagePoint::new(15.0, 5.0)];
    doc.set_freehand_points(id, moved.clone()).unwrap();
    assert!(matches!(
        doc.annotation(id),
        Some(Annotation::Freehand { kind: crate::FreehandKind::Highlighter, points, style: s, .. })
            if *points == moved && *s == style
    ));
    // No-op update commits no history entry.
    let before = doc.state_id();
    doc.set_freehand_points(id, moved).unwrap();
    assert_eq!(doc.state_id(), before);
}

#[test]
fn freehand_stroke_style_update_applies() {
    let mut doc = ImageDocument::new(RgbaImage::new(100, 100));
    let id = doc
        .add_freehand_with_style(
            crate::FreehandKind::Pen,
            vec![ImagePoint::new(0.0, 0.0), ImagePoint::new(10.0, 0.0)],
            StrokeStyle::default(),
        )
        .unwrap();
    let new_style = StrokeStyle {
        width: 8.0,
        ..StrokeStyle::default()
    };
    doc.set_stroke_style(id, new_style).unwrap();
    assert_eq!(doc.annotation(id).unwrap().stroke_style(), Some(new_style));
}
```

Also add lowering/bounds/hit/navigator tests:

In `shapes.rs` tests:

```rust
#[test]
fn freehand_lowers_to_polyline_with_opacity_alpha() {
    let pts = vec![ImagePoint::new(0.0, 0.0), ImagePoint::new(10.0, 5.0)];
    let a = Annotation::freehand(AnnotationId(1), crate::FreehandKind::Highlighter, pts.clone());
    let shapes = annotation_shapes(&a);
    assert_eq!(shapes.len(), 1);
    assert!(matches!(
        &shapes[0],
        RenderShape::Polyline { points, width, color }
            if *points == pts
                && *width == 12.0
                && color.a == (0.4_f32 * 255.0).round() as u8
    ));
}

#[test]
fn freehand_bounds_cover_points_plus_half_width() {
    let pts = vec![ImagePoint::new(10.0, 10.0), ImagePoint::new(30.0, 20.0)];
    let a = Annotation::freehand_with_style(
        AnnotationId(1),
        crate::FreehandKind::Pen,
        pts,
        StrokeStyle {
            width: 6.0,
            ..StrokeStyle::default()
        },
    );
    assert_eq!(
        annotation_bounds(&a),
        ImageRect {
            x: 7.0,
            y: 7.0,
            width: 26.0,
            height: 16.0
        }
    );
}
```

In `hit.rs` tests:

```rust
#[test]
fn freehand_hits_near_path_not_in_empty_bbox_corner() {
    let a = Annotation::freehand_with_style(
        AnnotationId(1),
        crate::FreehandKind::Pen,
        vec![
            ImagePoint::new(0.0, 0.0),
            ImagePoint::new(100.0, 0.0),
            ImagePoint::new(100.0, 100.0),
        ],
        StrokeStyle::default(), // width 4
    );
    // On the path: Body.
    assert_eq!(
        hit_test_annotation(&a, ImagePoint::new(50.0, 0.0), 2.0),
        Some(HitPart::Body)
    );
    // Within width/2 + tolerance: Body.
    assert_eq!(
        hit_test_annotation(&a, ImagePoint::new(50.0, 3.5), 2.0),
        Some(HitPart::Body)
    );
    // Inside the AABB but far from the path: miss.
    assert_eq!(hit_test_annotation(&a, ImagePoint::new(20.0, 80.0), 2.0), None);
}
```

In `navigator.rs` tests:

```rust
#[test]
fn freehand_labels_are_pen_and_highlighter() {
    let pen = Annotation::freehand(
        AnnotationId(1),
        crate::FreehandKind::Pen,
        vec![ImagePoint::new(0.0, 0.0), ImagePoint::new(5.0, 5.0)],
    );
    let hl = Annotation::freehand(
        AnnotationId(2),
        crate::FreehandKind::Highlighter,
        vec![ImagePoint::new(0.0, 10.0), ImagePoint::new(5.0, 15.0)],
    );
    let items = navigator_items(&[pen, hl]);
    assert_eq!(items[0].label, "Pen");
    assert_eq!(items[1].label, "Highlighter");
}
```

Run: `rtk cargo test -p rollshot-image-document freehand`
Expected: FAIL — `FreehandKind`, `Annotation::Freehand`, wrappers not defined.

- [ ] **Step 2: Add the model**

`annotation.rs` — after `ShapeKind`:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
pub enum FreehandKind {
    Pen,
    Highlighter,
}
```

Variant (after `Shape`):

```rust
    Freehand {
        id: AnnotationId,
        kind: FreehandKind,
        /// Simplified polyline in full-resolution image coordinates,
        /// stroke start → end.
        points: Vec<ImagePoint>,
        style: StrokeStyle,
    },
```

Constructors (after `shape_with_style`):

```rust
    pub fn freehand(id: AnnotationId, kind: FreehandKind, points: Vec<ImagePoint>) -> Self {
        let style = match kind {
            FreehandKind::Pen => StrokeStyle::default(),
            FreehandKind::Highlighter => StrokeStyle::highlighter_default(),
        };
        Self::freehand_with_style(id, kind, points, style)
    }

    pub fn freehand_with_style(
        id: AnnotationId,
        kind: FreehandKind,
        points: Vec<ImagePoint>,
        style: StrokeStyle,
    ) -> Self {
        Self::Freehand {
            id,
            kind,
            points,
            style,
        }
    }
```

Accessor arms:

- `id()`: add `| Annotation::Freehand { id, .. }` to the or-pattern.
- `anchor()`:

```rust
            Annotation::Freehand { points, .. } => ImagePoint::new(
                points.iter().map(|p| p.x).fold(f32::MAX, f32::min),
                points.iter().map(|p| p.y).fold(f32::MAX, f32::min),
            ),
```

- `stroke_style()`: add `Annotation::Freehand { style, .. } => Some(*style),`.

`style.rs` — after `impl Default for StrokeStyle`:

```rust
impl StrokeStyle {
    /// Reviewed Highlighter defaults (Slice 4 spec §4): highlighter yellow,
    /// triple pen width, uniform 40% alpha.
    pub fn highlighter_default() -> Self {
        Self {
            color: Rgb8::new(0xFF, 0xD4, 0x00),
            width: 12.0,
            opacity: 0.4,
        }
    }
}
```

`lib.rs` — re-export `FreehandKind` where `ShapeKind`/`TwoPointKind` are re-exported.

- [ ] **Step 3: Edit ops and validation**

`edit_op.rs` — extend imports with `FreehandKind`; add ops:

```rust
    AddFreehand {
        kind: FreehandKind,
        points: Vec<ImagePoint>,
        style: StrokeStyle,
    },
    UpdateFreehandPoints {
        id: AnnotationId,
        points: Vec<ImagePoint>,
    },
```

`document.rs`:

- `EditError` variant:

```rust
    #[error("freehand strokes require at least two distinct finite points")]
    InvalidFreehandPath,
```

- Validation helper after `clamp_two_point`:

```rust
fn clamp_freehand_points(
    points: Vec<ImagePoint>,
    width: u32,
    height: u32,
) -> Result<Vec<ImagePoint>, EditError> {
    if points.len() < 2 {
        return Err(EditError::InvalidFreehandPath);
    }
    for p in &points {
        ensure_point_finite(p)?;
    }
    let clamped: Vec<ImagePoint> = points.into_iter().map(|p| p.clamp_to(width, height)).collect();
    if clamped.iter().all(|p| *p == clamped[0]) {
        return Err(EditError::InvalidFreehandPath);
    }
    Ok(clamped)
}
```

- `apply_batch` referenced-id match: add `| EditOp::UpdateFreehandPoints { id, .. }` to the Some-arm and `| EditOp::AddFreehand { .. }` to the None-arm.
- `apply_one` arms (after `AddShape`-related arms):

```rust
            EditOp::AddFreehand {
                kind,
                points,
                style,
            } => {
                validate_stroke_style(style)?;
                let points = clamp_freehand_points(points, w, h)?;
                let id = self.allocate_id();
                self.annotations
                    .push(Annotation::freehand_with_style(id, kind, points, style));
                added_ids.push(id);
            }
            EditOp::UpdateFreehandPoints { id, points } => {
                let points = clamp_freehand_points(points, w, h)?;
                let index = self.annotation_index(id)?;
                match &mut self.annotations[index] {
                    Annotation::Freehand { points: p, .. } => *p = points,
                    _ => return Err(EditError::WrongKind),
                }
            }
```

- Extend the existing `EditOp::UpdateStrokeStyle` arm in `apply_one` so its inner annotation match accepts `Annotation::TwoPoint { style: s, .. } | Annotation::Freehand { style: s, .. }` (read the current arm first and keep its validation order).
- Wrappers (after `set_shape_style`):

```rust
    pub fn add_freehand_with_style(
        &mut self,
        kind: crate::annotation::FreehandKind,
        points: Vec<ImagePoint>,
        style: StrokeStyle,
    ) -> Result<AnnotationId, EditError> {
        let outcome = self.apply_batch(vec![EditOp::AddFreehand {
            kind,
            points,
            style,
        }])?;
        Ok(outcome.added_ids[0])
    }

    pub fn set_freehand_points(
        &mut self,
        id: AnnotationId,
        points: Vec<ImagePoint>,
    ) -> Result<(), EditError> {
        self.apply_batch(vec![EditOp::UpdateFreehandPoints { id, points }])?;
        Ok(())
    }
```

- [ ] **Step 4: Consumer arms in the document crate**

`shapes.rs` `annotation_shapes` (after the `Shape` arm):

```rust
        Annotation::Freehand { points, style, .. } => {
            let alpha = (style.opacity * 255.0).round() as u8;
            vec![RenderShape::Polyline {
                points: points.clone(),
                width: style.width,
                color: style.color.with_alpha(alpha),
            }]
        }
```

`shapes.rs` `annotation_bounds`:

```rust
        Annotation::Freehand { points, style, .. } => {
            crate::freehand::freehand_bounds(points, style.width)
        }
```

`hit.rs` `hit_test_annotation` (after the `Shape` arm):

```rust
        Annotation::Freehand { points, style, .. } => {
            (crate::freehand::polyline_distance(point, points) <= style.width / 2.0 + tolerance)
                .then_some(HitPart::Body)
        }
```

`navigator.rs` `label`:

```rust
        Annotation::Freehand { kind, .. } => match kind {
            crate::annotation::FreehandKind::Pen => "Pen".to_string(),
            crate::annotation::FreehandKind::Highlighter => "Highlighter".to_string(),
        },
```

- [ ] **Step 5: Compile-required app arms**

`canvas.rs` `draw_selection_handles` (after the `Shape` arm) — a freehand stroke has no handles; show a bounding-box outline like TextNote:

```rust
            Annotation::Freehand { points, style, .. } => {
                let b = rollshot_image_document::annotation_bounds(annotation);
                let _ = (points, style);
                frame.stroke(
                    &canvas::Path::rectangle(
                        Point::new(b.x * s, b.y * s),
                        Size::new(b.width * s, b.height * s),
                    ),
                    canvas::Stroke::default().with_color(accent).with_width(2.0),
                );
            }
```

(If `annotation_bounds` is already imported at `canvas.rs:244`, use the plain name and drop the `let _`; bind fields with `..` instead.)

`properties.rs` `preview_annotation` (after the `Shape` arm) — placeholder until Task 9:

```rust
        Annotation::Freehand { .. } => None,
```

`update.rs` `handle_canvas_released` `EditAnnotation` result match (after the `Shape` arm):

```rust
                    Annotation::Freehand { points, .. } => state
                        .document
                        .image
                        .set_freehand_points(original.id(), points.clone()),
```

- [ ] **Step 6: Sweep remaining exhaustive matches**

Run: `rtk cargo build --workspace`
For every `non-exhaustive patterns` error the compiler reports (automation proposal lowering, workbench review, Timeline, Action Guide render consumers), add the minimal arm consistent with that consumer's role — display passthrough or explicit "unsupported" — and note each file touched in the commit body. Do NOT add freehand creation to non-Result-Workspace consumers (spec §11).

- [ ] **Step 7: Run the full suite**

Run: `rtk cargo test`
Expected: all Task 3 tests pass; no regressions beyond the 3 known pre-existing failures.

- [ ] **Step 8: Commit**

```bash
rtk git add -A
rtk git commit -m "feat(annotation): add Freehand document model with edit ops and consumer arms"
```

---

### Task 4: App-side sampling, RDP simplification, and gesture helpers

**Files:**
- Create: `crates/rollshot-app/src/result_workspace/freehand_tool.rs`
- Modify: `crates/rollshot-app/src/result_workspace/mod.rs` (register `mod freehand_tool;` next to `mod box_tool;` / `mod two_point;`)

**Interfaces:**
- Produces:
  - `pub const MIN_SAMPLE_DISTANCE_SCREEN: f32 = 2.0;`
  - `pub const RDP_EPSILON_SCREEN: f32 = 1.0;`
  - `pub fn should_accept_point(last: ImagePoint, candidate: ImagePoint, scale: f32) -> bool`
  - `pub fn simplify_rdp(points: &[ImagePoint], epsilon: f32) -> Vec<ImagePoint>`
  - `pub fn path_meets_threshold(points: &[ImagePoint], scale: f32) -> bool` (larger bbox dimension × scale ≥ 4.0, reusing `two_point::MIN_GESTURE_SCREEN`)
  - `pub fn translated_points(points: &[ImagePoint], point: ImagePoint, grab_offset: (f32, f32), width: u32, height: u32) -> Vec<ImagePoint>` (rigid translation, bbox clamped to source, no deformation)
- Consumes: `rollshot_image_document::ImagePoint`, `super::two_point::MIN_GESTURE_SCREEN`.

- [ ] **Step 1: Write the failing tests**

Create the module with tests:

```rust
//! Freehand gesture helpers: pointer sampling filter, commit-time RDP
//! simplification, minimum-gesture rule, and rigid body movement
//! (Slice 4 spec §7). All screen-space thresholds divide by the viewport
//! scale so behavior is zoom-independent.

use rollshot_image_document::ImagePoint;

use super::two_point::MIN_GESTURE_SCREEN;

/// A new pointer sample must travel at least this many SCREEN pixels from
/// the last accepted point (spec §7.1).
pub const MIN_SAMPLE_DISTANCE_SCREEN: f32 = 2.0;
/// Ramer–Douglas–Peucker epsilon in SCREEN pixels (spec §7.2).
pub const RDP_EPSILON_SCREEN: f32 = 1.0;

pub fn should_accept_point(last: ImagePoint, candidate: ImagePoint, scale: f32) -> bool {
    last.distance(candidate) * scale >= MIN_SAMPLE_DISTANCE_SCREEN
}

/// Perpendicular distance from `p` to the infinite line through `a`..`b`
/// (or point distance when a == b).
fn line_distance(p: ImagePoint, a: ImagePoint, b: ImagePoint) -> f32 {
    let dx = b.x - a.x;
    let dy = b.y - a.y;
    let len = (dx * dx + dy * dy).sqrt();
    if len == 0.0 {
        return p.distance(a);
    }
    ((p.x - a.x) * dy - (p.y - a.y) * dx).abs() / len
}

/// Iterative Ramer–Douglas–Peucker. Keeps first and last points; the output
/// deviates from the input by at most `epsilon` (image-space units).
pub fn simplify_rdp(points: &[ImagePoint], epsilon: f32) -> Vec<ImagePoint> {
    if points.len() <= 2 {
        return points.to_vec();
    }
    let mut keep = vec![false; points.len()];
    keep[0] = true;
    keep[points.len() - 1] = true;
    let mut stack = vec![(0usize, points.len() - 1)];
    while let Some((first, last)) = stack.pop() {
        let mut max_d = 0.0f32;
        let mut index = first;
        for i in (first + 1)..last {
            let d = line_distance(points[i], points[first], points[last]);
            if d > max_d {
                max_d = d;
                index = i;
            }
        }
        if max_d > epsilon {
            keep[index] = true;
            stack.push((first, index));
            stack.push((index, last));
        }
    }
    points
        .iter()
        .zip(keep)
        .filter_map(|(p, k)| k.then_some(*p))
        .collect()
}

/// Minimum gesture: the larger bounding-box dimension must reach 4 screen
/// pixels. Uses one axis (not both) so a straight horizontal or vertical
/// stroke still commits (spec §7.2; differs from the box tool's two-axis
/// rule on purpose).
pub fn path_meets_threshold(points: &[ImagePoint], scale: f32) -> bool {
    if points.len() < 2 {
        return false;
    }
    let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
    for p in points {
        x0 = x0.min(p.x);
        y0 = y0.min(p.y);
        x1 = x1.max(p.x);
        y1 = y1.max(p.y);
    }
    (x1 - x0).max(y1 - y0) * scale >= MIN_GESTURE_SCREEN
}

/// Rigid translation of the whole path so its bounding box stays within the
/// source image. Mirrors the TwoPoint body-move clamp (no deformation).
pub fn translated_points(
    points: &[ImagePoint],
    point: ImagePoint,
    grab_offset: (f32, f32),
    width: u32,
    height: u32,
) -> Vec<ImagePoint> {
    let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
    for p in points {
        x0 = x0.min(p.x);
        y0 = y0.min(p.y);
        x1 = x1.max(p.x);
        y1 = y1.max(p.y);
    }
    let anchor = points[0];
    let dx = (point.x - grab_offset.0 - anchor.x).clamp(-x0, width as f32 - x1);
    let dy = (point.y - grab_offset.1 - anchor.y).clamp(-y0, height as f32 - y1);
    points
        .iter()
        .map(|p| ImagePoint::new(p.x + dx, p.y + dy))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sample_filter_is_zoom_independent() {
        let last = ImagePoint::new(0.0, 0.0);
        // 1.5 image px at scale 1.0 → below 2 screen px.
        assert!(!should_accept_point(last, ImagePoint::new(1.5, 0.0), 1.0));
        // Same image distance at scale 2.0 → 3 screen px, accepted.
        assert!(should_accept_point(last, ImagePoint::new(1.5, 0.0), 2.0));
    }

    #[test]
    fn rdp_collapses_collinear_points() {
        let pts: Vec<ImagePoint> = (0..=10)
            .map(|i| ImagePoint::new(i as f32, 0.0))
            .collect();
        assert_eq!(
            simplify_rdp(&pts, 1.0),
            vec![ImagePoint::new(0.0, 0.0), ImagePoint::new(10.0, 0.0)]
        );
    }

    #[test]
    fn rdp_preserves_corners_and_drops_small_wiggles() {
        // An L-shaped stroke with a 0.5-px wiggle on the horizontal leg:
        // the corner (10, 0) is kept (7.07 px off the end-to-end chord);
        // the wiggle (5, 0.5) is 0.5 px off the (0,0)-(10,0) sub-chord and
        // drops at epsilon 1.0.
        let pts = vec![
            ImagePoint::new(0.0, 0.0),
            ImagePoint::new(5.0, 0.5),
            ImagePoint::new(10.0, 0.0),
            ImagePoint::new(10.0, 10.0),
        ];
        let out = simplify_rdp(&pts, 1.0);
        assert_eq!(
            out,
            vec![
                ImagePoint::new(0.0, 0.0),
                ImagePoint::new(10.0, 0.0),
                ImagePoint::new(10.0, 10.0),
            ]
        );
    }

    #[test]
    fn rdp_output_within_epsilon_of_input() {
        let pts: Vec<ImagePoint> = (0..100)
            .map(|i| {
                let x = i as f32;
                ImagePoint::new(x, (x / 6.0).sin() * 20.0)
            })
            .collect();
        let out = simplify_rdp(&pts, 1.0);
        assert!(out.len() < pts.len());
        // Every dropped input point stays within epsilon of the output path.
        for p in &pts {
            let d = out
                .windows(2)
                .map(|w| {
                    // Reuse the same distance definition as the document hit
                    // path: clamped projection onto each output segment.
                    let dx = w[1].x - w[0].x;
                    let dy = w[1].y - w[0].y;
                    let len_sq = dx * dx + dy * dy;
                    let t = (((p.x - w[0].x) * dx + (p.y - w[0].y) * dy) / len_sq)
                        .clamp(0.0, 1.0);
                    p.distance(ImagePoint::new(w[0].x + t * dx, w[0].y + t * dy))
                })
                .fold(f32::MAX, f32::min);
            assert!(d <= 1.0 + 1e-3, "point deviates by {d}");
        }
    }

    #[test]
    fn short_strokes_survive_rdp() {
        let pts = vec![ImagePoint::new(0.0, 0.0), ImagePoint::new(3.0, 1.0)];
        assert_eq!(simplify_rdp(&pts, 1.0), pts);
    }

    #[test]
    fn threshold_uses_larger_dimension() {
        // Straight 5-px horizontal stroke (zero height) commits at scale 1.
        let flat = vec![ImagePoint::new(0.0, 0.0), ImagePoint::new(5.0, 0.0)];
        assert!(path_meets_threshold(&flat, 1.0));
        // 3-px stroke fails at scale 1, passes at scale 2.
        let tiny = vec![ImagePoint::new(0.0, 0.0), ImagePoint::new(3.0, 0.0)];
        assert!(!path_meets_threshold(&tiny, 1.0));
        assert!(path_meets_threshold(&tiny, 2.0));
    }

    #[test]
    fn translation_clamps_bbox_without_deforming() {
        let pts = vec![ImagePoint::new(10.0, 10.0), ImagePoint::new(20.0, 30.0)];
        // Drag far past the left edge: dx clamps to -10 (bbox min x → 0).
        let out = translated_points(&pts, ImagePoint::new(-100.0, 10.0), (0.0, 0.0), 100, 100);
        assert_eq!(out[0], ImagePoint::new(0.0, 10.0));
        assert_eq!(out[1], ImagePoint::new(10.0, 30.0));
        // Relative geometry preserved.
        assert_eq!(out[1].x - out[0].x, 10.0);
        assert_eq!(out[1].y - out[0].y, 20.0);
    }
}
```

- [ ] **Step 2: Register the module and run the tests**

Add `mod freehand_tool;` to `crates/rollshot-app/src/result_workspace/mod.rs` next to `mod box_tool;`.

Run: `rtk cargo test -p rollshot-app freehand_tool`
Expected: all 8 tests PASS.

- [ ] **Step 3: Commit**

```bash
rtk git add crates/rollshot-app/src/result_workspace/freehand_tool.rs crates/rollshot-app/src/result_workspace/mod.rs
rtk git commit -m "feat(annotation): add freehand sampling, RDP, and movement helpers"
```

---

### Task 5: `Tool::Pen` / `Tool::Highlighter`, toolbar, and shortcuts

**Files:**
- Modify: `crates/rollshot-app/src/result_workspace/canvas.rs` (`Tool` enum ~line 29)
- Modify: `crates/rollshot-app/src/result_workspace/toolbar.rs` (`tool_item` ~line 108, `toolbar_model` ~line 171, `tool_tooltip` ~line 469)
- Modify: `crates/rollshot-app/src/result_workspace/update.rs` (`map_key_press` ~line 2686, `direct_manipulation_hit` ~line 491)
- Modify: `crates/rollshot-app/src/result_workspace/properties.rs` (`property_target` ~line 81 — temporary `None` arms, upgraded in Task 9)

**Interfaces:**
- Produces: `Tool::Pen`, `Tool::Highlighter`; keyboard `p`/`h` → `Message::SelectTool(...)`; toolbar items labeled `Pen` (shortcut `P`) and `Highlighter` (shortcut `H`).
- Consumes: existing `Message::SelectTool` flow (generic — no new message needed).

**Invoke the `iced-rs` skill before toolbar view edits.**

- [ ] **Step 1: Write failing toolbar tests**

Append to the `#[cfg(test)] mod tests` in `toolbar.rs` (mirror the existing density tests at `toolbar.rs:766+`, which construct a `ResultWorkspace` test state — reuse their helper):

```rust
#[test]
fn pen_and_highlighter_route_by_density() {
    let state = test_state(); // reuse the existing test helper in this module
    // Wide: both visible.
    let wide = toolbar_model(&state, 1200.0);
    assert!(wide.visible_tools.contains(&Tool::Pen));
    assert!(wide.visible_tools.contains(&Tool::Highlighter));
    // Narrow: Pen visible, Highlighter in More (with Line and Redact).
    let narrow = toolbar_model(&state, 600.0);
    assert!(narrow.visible_tools.contains(&Tool::Pen));
    assert!(!narrow.visible_tools.contains(&Tool::Highlighter));
    assert!(narrow
        .more
        .iter()
        .any(|i| matches!(i.kind, ToolbarItemKind::Tool(Tool::Highlighter))));
}

#[test]
fn pen_and_highlighter_items_have_shortcuts() {
    assert_eq!(tool_item(Tool::Pen).shortcut, "P");
    assert_eq!(tool_item(Tool::Highlighter).shortcut, "H");
}
```

And a keyboard test in `update.rs` tests (mirror existing `map_key_press` tests):

```rust
#[test]
fn p_and_h_select_freehand_tools() {
    let p = keyboard::Key::Character("p".into());
    let h = keyboard::Key::Character("h".into());
    assert_eq!(
        map_key_press(&p, keyboard::Modifiers::default(), false),
        Some(Message::SelectTool(Tool::Pen))
    );
    assert_eq!(
        map_key_press(&h, keyboard::Modifiers::default(), false),
        Some(Message::SelectTool(Tool::Highlighter))
    );
    // Captured input ignores tool shortcuts.
    assert_eq!(map_key_press(&p, keyboard::Modifiers::default(), true), None);
}
```

Run: `rtk cargo test -p rollshot-app toolbar`
Expected: FAIL — `Tool::Pen` not defined.

- [ ] **Step 2: Implement**

`canvas.rs` `Tool` enum — add after `Ellipse`:

```rust
    Pen,
    Highlighter,
```

`toolbar.rs` `tool_item` — add arms:

```rust
        Tool::Pen => ToolbarItem {
            kind: ToolbarItemKind::Tool(Tool::Pen),
            label: "Pen",
            shortcut: "P",
        },
        Tool::Highlighter => ToolbarItem {
            kind: ToolbarItemKind::Tool(Tool::Highlighter),
            label: "Highlighter",
            shortcut: "H",
        },
```

`toolbar.rs` `toolbar_model` — umbrella second-row order (`… Shapes Pen Highlight Redact`):

- Wide/Compact `primary_tools`: `vec![Tool::Select, Tool::Number, Tool::Text, Tool::Line, Tool::Arrow, remembered.into(), Tool::Pen, Tool::Highlighter, Tool::Redact]`
- Narrow `primary_tools`: `vec![Tool::Select, Tool::Number, Tool::Text, Tool::Arrow, remembered.into(), Tool::Pen]`
- Narrow overflow inserts (keep More order Line, Highlighter, Redact):

```rust
    if density == ToolbarDensity::Narrow {
        overflow.insert(0, tool_item(Tool::Redact));
        overflow.insert(0, tool_item(Tool::Highlighter));
        overflow.insert(0, tool_item(Tool::Line));
    }
```

`tool_tooltip` — no change needed: Pen/Highlighter fall into the default arm producing `"Pen (P)"` / `"Highlighter (H)"`.

`update.rs` `map_key_press` — add to the non-command character match:

```rust
            "p" => Some(Message::SelectTool(Tool::Pen)),
            "h" => Some(Message::SelectTool(Tool::Highlighter)),
```

`update.rs` `direct_manipulation_hit` — extend the creation-tool arm:

```rust
        Tool::Number
        | Tool::Text
        | Tool::Line
        | Tool::Arrow
        | Tool::Rectangle
        | Tool::Ellipse
        | Tool::Pen
        | Tool::Highlighter => None,
```

`properties.rs` `property_target` — temporary arms (Task 9 replaces them):

```rust
        Tool::Pen | Tool::Highlighter => None,
```

- [ ] **Step 3: Build, run tests, commit**

Run: `rtk cargo test -p rollshot-app`
Expected: new tests PASS; existing toolbar tests still pass (some assert exact tool lists — update those assertions to include Pen/Highlighter where the spec's order requires it, and treat any OTHER behavior change as a bug in this task).

```bash
rtk git add crates/rollshot-app/src/result_workspace/canvas.rs crates/rollshot-app/src/result_workspace/toolbar.rs crates/rollshot-app/src/result_workspace/update.rs crates/rollshot-app/src/result_workspace/properties.rs
rtk git commit -m "feat(annotation): add Pen and Highlighter tools with toolbar routing and shortcuts"
```

---

### Task 6: Persisted Pen and Highlighter defaults

**Files:**
- Modify: `crates/rollshot-app/src/result_workspace/annotation_defaults.rs`

**Interfaces:**
- Produces: `AnnotationDefaults { …, pub pen: StrokeStyle, pub highlighter: StrokeStyle, … }`; canonical `pen = StrokeStyle::default()`, `highlighter = StrokeStyle::highlighter_default()`.
- Produces: `load_stroke_style` gains an `allow_translucent: bool` + per-key `StrokeStyle` defaults parameter; save keeps forcing `opacity = 1.0` for `line`, `arrow`, `pen`, `rectangle.stroke`, `ellipse.stroke` but persists highlighter opacity in `(0.0, 1.0]`.
- Consumes: `StrokeStyle::highlighter_default()` (Task 3).

- [ ] **Step 1: Write failing tests**

Append to the module's tests (mirror the round-trip tests at `annotation_defaults.rs:436+`, which use a temp dir):

```rust
#[test]
fn highlighter_opacity_round_trips_but_pen_is_forced_opaque() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    let mut values = AnnotationDefaults::default();
    values.highlighter.opacity = 0.25;
    values.pen.opacity = 0.5; // must NOT survive persistence
    save_to(&path, &values).unwrap();
    let loaded = load_from(&path);
    assert_eq!(loaded.values.highlighter.opacity, 0.25);
    assert_eq!(loaded.values.pen.opacity, 1.0);
    assert!(loaded.warnings.is_empty());
}

#[test]
fn invalid_highlighter_opacity_falls_back_with_warning() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    std::fs::write(
        &path,
        "[annotation_defaults.highlighter]\nopacity = 1.7\n",
    )
    .unwrap();
    let loaded = load_from(&path);
    assert_eq!(loaded.values.highlighter.opacity, 0.4);
    assert_eq!(loaded.warnings.len(), 1);
}

#[test]
fn missing_freehand_sections_resolve_to_canonical_defaults() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    std::fs::write(&path, "[annotation_defaults]\n").unwrap();
    let loaded = load_from(&path);
    assert_eq!(loaded.values.pen, StrokeStyle::default());
    assert_eq!(loaded.values.highlighter, StrokeStyle::highlighter_default());
}
```

(If the existing tests use a different temp-file helper than `tempfile`, reuse that helper instead.)

Run: `rtk cargo test -p rollshot-app annotation_defaults`
Expected: FAIL — no `pen`/`highlighter` fields.

- [ ] **Step 2: Implement**

- Add fields to `AnnotationDefaults` and its `Default` impl:

```rust
    pub pen: StrokeStyle,
    pub highlighter: StrokeStyle,
    // in Default::default():
    pen: StrokeStyle::default(),
    highlighter: StrokeStyle::highlighter_default(),
```

- Change `load_stroke_style` signature to `fn load_stroke_style(parent: &toml::Table, key: &str, defaults: StrokeStyle, allow_translucent: bool, warnings: &mut Vec<String>) -> StrokeStyle` and replace the hardcoded `StrokeStyle::default()` fallbacks with `defaults`. Replace the opacity block with:

```rust
    let opacity = match table.get("opacity") {
        None => defaults.opacity,
        Some(value) => match value.as_float().map(|o| o as f32) {
            Some(o) if allow_translucent && o.is_finite() && o > 0.0 && o <= 1.0 => o,
            Some(1.0) => 1.0,
            _ => {
                invalid = true;
                defaults.opacity
            }
        },
    };
```

Note: without `allow_translucent`, any value other than exactly `1.0` stays invalid — preserving the existing behavior for every non-highlighter key.

- Update all `load_stroke_style` call sites: `line`/`arrow` pass `(StrokeStyle::default(), false)`; `load_shape_defaults`'s inner call passes `(StrokeStyle::default(), false)`; add in `load_from`:

```rust
    let pen = load_stroke_style(&section, "pen", StrokeStyle::default(), false, &mut warnings);
    let highlighter = load_stroke_style(
        &section,
        "highlighter",
        StrokeStyle::highlighter_default(),
        true,
        &mut warnings,
    );
```

and thread both into the returned `AnnotationDefaults`.

- In `save_to_with_writer`, extend the force-opaque block:

```rust
    persisted.pen.opacity = 1.0;
    // Highlighter is the one persisted translucent stroke (Slice 4 spec §9.2).
    if !(persisted.highlighter.opacity.is_finite()
        && persisted.highlighter.opacity > 0.0
        && persisted.highlighter.opacity <= 1.0)
    {
        persisted.highlighter.opacity = StrokeStyle::highlighter_default().opacity;
    }
```

- [ ] **Step 3: Run tests and commit**

Run: `rtk cargo test -p rollshot-app annotation_defaults`
Expected: PASS (including the pre-existing round-trip tests — the widened signature must not change their outcomes).

```bash
rtk git add crates/rollshot-app/src/result_workspace/annotation_defaults.rs
rtk git commit -m "feat(annotation): persist pen and highlighter defaults with translucent highlighter opacity"
```

---

### Task 7: Freehand creation gesture (draft, filter, simplify, commit) and body movement

**Files:**
- Modify: `crates/rollshot-app/src/result_workspace/canvas.rs` (`DragState::CreateFreehand`, `draft_annotation` arm, `dragged_annotation` freehand-Body arm)
- Modify: `crates/rollshot-app/src/result_workspace/update.rs` (`handle_canvas_pressed`, `handle_canvas_moved`, `handle_canvas_released`, `grab_offset`, `active_freehand` helper)

**Interfaces:**
- Produces: `DragState::CreateFreehand { kind: FreehandKind, points: Vec<ImagePoint>, style: StrokeStyle }`.
- Consumes: Task 4 helpers, `ImageDocument::add_freehand_with_style` / `set_freehand_points` (Task 3), `AnnotationDefaults.pen`/`highlighter` (Task 6).

- [ ] **Step 1: Write failing tests**

In `canvas.rs` tests (mirror `dragged_annotation` tests at ~line 811):

```rust
#[test]
fn dragged_freehand_body_translates_all_points_with_clamp() {
    let original = Annotation::freehand(
        AnnotationId(1),
        rollshot_image_document::FreehandKind::Pen,
        vec![ImagePoint::new(10.0, 10.0), ImagePoint::new(20.0, 30.0)],
    );
    let next = dragged_annotation(
        &original,
        HitPart::Body,
        ImagePoint::new(15.0, 15.0),
        (0.0, 0.0), // grab at points[0]
        false,
        (100, 100),
        1.0,
    );
    match next {
        Annotation::Freehand { points, .. } => {
            assert_eq!(points[0], ImagePoint::new(15.0, 15.0));
            assert_eq!(points[1], ImagePoint::new(25.0, 35.0));
        }
        _ => panic!("expected freehand"),
    }
}
```

In `update.rs` tests (mirror the existing gesture tests; they build a `ResultWorkspace` via a test helper — reuse it):

```rust
#[test]
fn freehand_gesture_filters_samples_and_commits_simplified_stroke() {
    let mut state = test_workspace(); // reuse the module's existing helper
    state.editor.tool = Tool::Pen;
    let t0 = std::time::Instant::now();
    let _ = handle_canvas_pressed(&mut state, ImagePoint::new(10.0, 10.0), t0);
    // 30 collinear moves 1px apart: the 2-screen-px filter drops half, and
    // RDP collapses the rest to the two endpoints.
    for i in 1..=30 {
        let _ = handle_canvas_moved(&mut state, ImagePoint::new(10.0 + i as f32, 10.0));
    }
    let _ = handle_canvas_released(&mut state, ImagePoint::new(40.0, 10.0));
    let annotations = state.document.image.annotations();
    assert_eq!(annotations.len(), 1);
    match &annotations[0] {
        Annotation::Freehand { kind, points, .. } => {
            assert_eq!(*kind, rollshot_image_document::FreehandKind::Pen);
            assert_eq!(points.len(), 2, "collinear stroke must simplify to endpoints");
            assert_eq!(points[0], ImagePoint::new(10.0, 10.0));
            assert_eq!(points[1], ImagePoint::new(40.0, 10.0));
        }
        other => panic!("expected freehand, got {other:?}"),
    }
    // One gesture → one history entry.
    assert!(state.document.image.can_undo());
    state.document.image.undo();
    assert!(!state.document.image.can_undo());
}

#[test]
fn freehand_click_and_subthreshold_gestures_cancel() {
    let mut state = test_workspace();
    state.editor.tool = Tool::Highlighter;
    let t0 = std::time::Instant::now();
    // Plain click.
    let _ = handle_canvas_pressed(&mut state, ImagePoint::new(10.0, 10.0), t0);
    let _ = handle_canvas_released(&mut state, ImagePoint::new(10.0, 10.0));
    // 2-px wiggle (below the 4-screen-px threshold at scale 1).
    let _ = handle_canvas_pressed(&mut state, ImagePoint::new(10.0, 10.0), t0);
    let _ = handle_canvas_moved(&mut state, ImagePoint::new(12.0, 10.0));
    let _ = handle_canvas_released(&mut state, ImagePoint::new(12.0, 10.0));
    assert!(state.document.image.annotations().is_empty());
    assert!(!state.document.image.can_undo());
}

#[test]
fn highlighter_stroke_uses_highlighter_defaults() {
    let mut state = test_workspace();
    state.editor.tool = Tool::Highlighter;
    let t0 = std::time::Instant::now();
    let _ = handle_canvas_pressed(&mut state, ImagePoint::new(10.0, 10.0), t0);
    let _ = handle_canvas_moved(&mut state, ImagePoint::new(40.0, 20.0));
    let _ = handle_canvas_released(&mut state, ImagePoint::new(40.0, 20.0));
    match &state.document.image.annotations()[0] {
        Annotation::Freehand { style, .. } => {
            assert_eq!(*style, StrokeStyle::highlighter_default());
        }
        other => panic!("expected freehand, got {other:?}"),
    }
}
```

Run: `rtk cargo test -p rollshot-app freehand`
Expected: FAIL — `CreateFreehand` not defined.

- [ ] **Step 2: Implement the draft state and canvas arms**

`canvas.rs` `DragState` — add after `CreateShape`:

```rust
    /// Pen/Highlighter: the first accumulating draft. `points` holds the
    /// distance-filtered raw samples; RDP simplification runs once on release
    /// (spec §7.1/§7.2).
    CreateFreehand {
        kind: rollshot_image_document::FreehandKind,
        points: Vec<ImagePoint>,
        style: StrokeStyle,
    },
```

`canvas.rs` `draft_annotation` — add before the `EditAnnotation` arm:

```rust
            Some(DragState::CreateFreehand {
                kind,
                points,
                style,
            }) => (points.len() >= 2).then(|| {
                Annotation::freehand_with_style(
                    AnnotationId(u64::MAX),
                    *kind,
                    points.clone(),
                    *style,
                )
            }),
```

`canvas.rs` `dragged_annotation` — add before the wildcard arm:

```rust
        (Annotation::Freehand { points, .. }, HitPart::Body) => {
            *points = super::freehand_tool::translated_points(
                points,
                point,
                grab_offset,
                width,
                height,
            );
        }
```

- [ ] **Step 3: Implement the pointer handlers**

`update.rs` — helper next to `active_two_point` (~line 386):

```rust
fn active_freehand(
    state: &super::ResultWorkspace,
) -> Option<(rollshot_image_document::FreehandKind, StrokeStyle)> {
    match state.editor.tool {
        Tool::Pen => Some((
            rollshot_image_document::FreehandKind::Pen,
            state.annotation_defaults.values.pen,
        )),
        Tool::Highlighter => Some((
            rollshot_image_document::FreehandKind::Highlighter,
            state.annotation_defaults.values.highlighter,
        )),
        _ => None,
    }
}
```

`handle_canvas_pressed` — add arm after `Tool::Rectangle | Tool::Ellipse`:

```rust
        Tool::Pen | Tool::Highlighter => {
            let (kind, style) = active_freehand(state)
                .expect("pen and highlighter tools always provide freehand defaults");
            let (w, h) = state.document.image.source().dimensions();
            state.editor.drag = Some(DragState::CreateFreehand {
                kind,
                points: vec![point.clamp_to(w, h)],
                style,
            });
            Task::none()
        }
```

`handle_canvas_moved` — add arm after `CreateShape`:

```rust
        Some(DragState::CreateFreehand { points, .. }) => {
            if let Some(last) = points.last().copied() {
                if super::freehand_tool::should_accept_point(last, point, scale) {
                    points.push(point);
                }
            }
            Task::none()
        }
```

(`point` is already clamped to image bounds at the top of `handle_canvas_moved`.)

`handle_canvas_released` — add arm after `CreateShape`:

```rust
        Some(DragState::CreateFreehand {
            kind,
            mut points,
            style,
        }) => {
            if points.last() != Some(&point) {
                points.push(point);
            }
            let input_points = points.len();
            let epsilon = super::freehand_tool::RDP_EPSILON_SCREEN / scale;
            let simplified = super::freehand_tool::simplify_rdp(&points, epsilon);
            tracing::debug!(
                target: "rollshot::annotation",
                input_points,
                output_points = simplified.len(),
                kind = ?kind,
                "freehand simplification"
            );
            if super::freehand_tool::path_meets_threshold(&simplified, scale) {
                if let Err(error) =
                    state
                        .document
                        .image
                        .add_freehand_with_style(kind, simplified, style)
                {
                    state.message = Some(InlineMessage::Error(error.to_string()));
                }
            }
        }
```

`grab_offset` (~line 452) — add before the wildcard:

```rust
        (Annotation::Freehand { points, .. }, HitPart::Body) => {
            (point.x - points[0].x, point.y - points[0].y)
        }
```

Esc cancellation needs no new code: verify `Message::EscapePressed` handling already clears `state.editor.drag` for any active drag (read the existing handler; if it special-cases drag kinds, extend it — otherwise leave untouched).

- [ ] **Step 4: Add flatten-cost tracing**

In `update.rs` `copy_payload` and `save_payload` (~line 505), wrap the flatten call:

```rust
pub(crate) fn copy_payload(state: &super::ResultWorkspace) -> RgbaImage {
    let started = std::time::Instant::now();
    let out = state.document.image.flatten();
    tracing::debug!(
        target: "rollshot::annotation",
        elapsed_ms = started.elapsed().as_millis() as u64,
        annotations = state.document.image.annotations().len(),
        "flatten for copy"
    );
    out
}
```

(Same pattern in `save_payload` around its `flatten()` branch, message `"flatten for save"`.)

- [ ] **Step 5: Run tests and commit**

Run: `rtk cargo test -p rollshot-app`
Expected: the 4 new tests PASS; no gesture regressions.

```bash
rtk git add crates/rollshot-app/src/result_workspace/canvas.rs crates/rollshot-app/src/result_workspace/update.rs
rtk git commit -m "feat(annotation): freehand creation gesture with sampling filter and RDP commit"
```

---

### Task 8: Uniform-alpha output-lifecycle tests (document crate)

Verify the spec's §12.4 output rules end-to-end through `ImageDocument::flatten` (Task 2 tested the rasterizer directly; this task tests through the committed document).

**Files:**
- Modify: `crates/rollshot-image-document/src/flatten.rs` (tests only)

- [ ] **Step 1: Write the tests (they should pass immediately — they verify integration, not new code)**

```rust
#[test]
fn committed_highlighter_flattens_with_uniform_alpha() {
    let mut doc = crate::ImageDocument::new(RgbaImage::from_pixel(
        60,
        60,
        image::Rgba([255, 255, 255, 255]),
    ));
    // Self-crossing highlighter stroke.
    doc.add_freehand_with_style(
        crate::FreehandKind::Highlighter,
        vec![
            ImagePoint::new(10.0, 10.0),
            ImagePoint::new(50.0, 50.0),
            ImagePoint::new(10.0, 50.0),
            ImagePoint::new(50.0, 10.0),
        ],
        crate::StrokeStyle::highlighter_default(),
    )
    .unwrap();
    let out = doc.flatten();
    let crossing = out.get_pixel(30, 30).0;
    let single = out.get_pixel(15, 15).0;
    assert_eq!(crossing, single, "self-overlap must not darken");
    // 0.4 alpha of #FFD400 over white: blue channel drops the most.
    assert!(crossing[2] < 255);
    // Source is untouched.
    assert_eq!(doc.source().get_pixel(30, 30).0, [255, 255, 255, 255]);
}

#[test]
fn committed_pen_flattens_opaque() {
    let mut doc = crate::ImageDocument::new(RgbaImage::from_pixel(
        40,
        40,
        image::Rgba([255, 255, 255, 255]),
    ));
    doc.add_freehand_with_style(
        crate::FreehandKind::Pen,
        vec![ImagePoint::new(5.0, 20.0), ImagePoint::new(35.0, 20.0)],
        crate::StrokeStyle::default(),
    )
    .unwrap();
    let out = doc.flatten();
    // Fully covered center pixel is exactly the stroke color.
    assert_eq!(out.get_pixel(20, 20).0, [0xE5, 0x48, 0x4D, 255]);
}
```

- [ ] **Step 2: Run and commit**

Run: `rtk cargo test -p rollshot-image-document flatten`
Expected: PASS.

```bash
rtk git add crates/rollshot-image-document/src/flatten.rs
rtk git commit -m "test(annotation): freehand uniform-alpha output lifecycle"
```

---

### Task 9: Contextual properties — stroke controls plus the Highlighter opacity slider

**Files:**
- Modify: `crates/rollshot-app/src/result_workspace/properties.rs` (`PropertyTarget::FreehandTool`, `property_target`, `stroke_width`, controls, `preview_annotation`)
- Modify: `crates/rollshot-app/src/result_workspace/update.rs` (`Message::PreviewStrokeOpacity` / `Message::ApplyStrokeOpacity`, color/width arm extensions, `clear_property_transactions`, Undo guard)

**Interfaces:**
- Produces:
  - `PropertyTarget::FreehandTool(FreehandKind)`
  - `pub struct OpacityTransaction { pub target: PropertyTarget, pub original: f32, pub preview: f32 }` and `PropertyState.opacity: Option<OpacityTransaction>`
  - `Message::PreviewStrokeOpacity(f32)`, `Message::ApplyStrokeOpacity` (mirror `PreviewStrokeWidth`/`ApplyStrokeWidth` including the `Message` enum and its PartialEq arm at `update.rs:251`)
- Consumes: `preview_annotation` consumption in `canvas.rs` (already wired), defaults from Task 6.

- [ ] **Step 1: Write failing tests**

In `properties.rs` tests:

```rust
#[test]
fn pen_and_highlighter_have_freehand_targets() {
    let mut state = test_workspace(); // reuse the module's existing helper
    state.editor.tool = Tool::Pen;
    assert_eq!(
        property_target(&state),
        Some(PropertyTarget::FreehandTool(
            rollshot_image_document::FreehandKind::Pen
        ))
    );
    state.editor.tool = Tool::Highlighter;
    assert_eq!(
        property_target(&state),
        Some(PropertyTarget::FreehandTool(
            rollshot_image_document::FreehandKind::Highlighter
        ))
    );
}

#[test]
fn selected_freehand_targets_annotation() {
    let mut state = test_workspace();
    let id = state
        .document
        .image
        .add_freehand_with_style(
            rollshot_image_document::FreehandKind::Pen,
            vec![ImagePoint::new(0.0, 0.0), ImagePoint::new(10.0, 10.0)],
            StrokeStyle::default(),
        )
        .unwrap();
    state.editor.tool = Tool::Select;
    state.editor.selection = Some(id);
    assert_eq!(property_target(&state), Some(PropertyTarget::Annotation(id)));
}

#[test]
fn freehand_preview_applies_width_color_and_opacity_transactions() {
    let mut state = test_workspace();
    let id = state
        .document
        .image
        .add_freehand_with_style(
            rollshot_image_document::FreehandKind::Highlighter,
            vec![ImagePoint::new(0.0, 0.0), ImagePoint::new(10.0, 10.0)],
            StrokeStyle::highlighter_default(),
        )
        .unwrap();
    state.editor.tool = Tool::Select;
    state.editor.selection = Some(id);
    state.editor.properties.opacity = Some(OpacityTransaction {
        target: PropertyTarget::Annotation(id),
        original: 0.4,
        preview: 0.8,
    });
    match preview_annotation(&state) {
        Some(Annotation::Freehand { style, .. }) => assert_eq!(style.opacity, 0.8),
        other => panic!("expected freehand preview, got {other:?}"),
    }
}
```

In `update.rs` tests:

```rust
#[test]
fn apply_opacity_to_highlighter_defaults_persists() {
    let mut state = test_workspace();
    state.editor.tool = Tool::Highlighter;
    let _ = update(&mut state, Message::PreviewStrokeOpacity(0.7));
    let _ = update(&mut state, Message::ApplyStrokeOpacity);
    assert_eq!(state.annotation_defaults.values.highlighter.opacity, 0.7);
}

#[test]
fn apply_opacity_to_selected_highlighter_is_one_undo_step() {
    let mut state = test_workspace();
    let id = state
        .document
        .image
        .add_freehand_with_style(
            rollshot_image_document::FreehandKind::Highlighter,
            vec![ImagePoint::new(0.0, 0.0), ImagePoint::new(10.0, 10.0)],
            StrokeStyle::highlighter_default(),
        )
        .unwrap();
    state.editor.tool = Tool::Select;
    state.editor.selection = Some(id);
    let _ = update(&mut state, Message::PreviewStrokeOpacity(0.9));
    let _ = update(&mut state, Message::ApplyStrokeOpacity);
    assert_eq!(
        state.document.image.annotation(id).unwrap().stroke_style().unwrap().opacity,
        0.9
    );
    state.document.image.undo();
    assert_eq!(
        state.document.image.annotation(id).unwrap().stroke_style().unwrap().opacity,
        0.4
    );
}

#[test]
fn opacity_never_targets_pen() {
    let mut state = test_workspace();
    state.editor.tool = Tool::Pen;
    let _ = update(&mut state, Message::PreviewStrokeOpacity(0.5));
    assert!(state.editor.properties.opacity.is_none());
    let _ = update(&mut state, Message::ApplyStrokeOpacity);
    assert_eq!(state.annotation_defaults.values.pen.opacity, 1.0);
}
```

Run: `rtk cargo test -p rollshot-app properties`
Expected: FAIL.

- [ ] **Step 2: Implement `properties.rs`**

- Add `FreehandTool(rollshot_image_document::FreehandKind)` to `PropertyTarget` (import `FreehandKind`).
- Add `OpacityTransaction` after `StrokeWidthTransaction`:

```rust
#[derive(Debug, Clone, PartialEq)]
pub struct OpacityTransaction {
    pub target: PropertyTarget,
    pub original: f32,
    pub preview: f32,
}
```

- Add `pub opacity: Option<OpacityTransaction>,` to `PropertyState`.
- `property_target`: replace the Task 5 placeholder with

```rust
        Tool::Pen => Some(PropertyTarget::FreehandTool(
            rollshot_image_document::FreehandKind::Pen,
        )),
        Tool::Highlighter => Some(PropertyTarget::FreehandTool(
            rollshot_image_document::FreehandKind::Highlighter,
        )),
```

and add `Annotation::Freehand { .. }` to the Select-selection annotation list (~line 94-98).

- `stroke_width`: add

```rust
        PropertyTarget::FreehandTool(kind) => Some(match kind {
            rollshot_image_document::FreehandKind::Pen => {
                state.annotation_defaults.values.pen.width
            }
            rollshot_image_document::FreehandKind::Highlighter => {
                state.annotation_defaults.values.highlighter.width
            }
        }),
```

- New helper for the opacity slider (10%–100%, step 0.05, mirror `stroke_controls`):

```rust
fn opacity_control(
    state: &ResultWorkspace,
    target: PropertyTarget,
    committed: f32,
) -> Element<'static, Message> {
    use iced::widget::{row, slider, text};
    let value = state
        .editor
        .properties
        .opacity
        .as_ref()
        .filter(|tx| tx.target == target)
        .map(|tx| tx.preview)
        .unwrap_or(committed);
    row![
        text(format!("{:.0}%", value * 100.0)).size(12),
        slider(0.1_f32..=1.0_f32, value, Message::PreviewStrokeOpacity)
            .step(0.05_f32)
            .on_release(Message::ApplyStrokeOpacity)
            .width(96),
    ]
    .spacing(4)
    .into()
}
```

- `view`: add target arms —

```rust
        PropertyTarget::FreehandTool(kind) => {
            let stroke = stroke_controls(state, target)?;
            match kind {
                rollshot_image_document::FreehandKind::Pen => Some(stroke),
                rollshot_image_document::FreehandKind::Highlighter => {
                    let committed = state.annotation_defaults.values.highlighter.opacity;
                    Some(
                        iced::widget::row![stroke, opacity_control(state, target, committed)]
                            .spacing(8)
                            .into(),
                    )
                }
            }
        }
```

and in the `PropertyTarget::Annotation(id)` match add before the final `_ => None`:

```rust
            Annotation::Freehand { kind, style, .. } => {
                let stroke = stroke_controls(state, target)?;
                match kind {
                    rollshot_image_document::FreehandKind::Pen => Some(stroke),
                    rollshot_image_document::FreehandKind::Highlighter => Some(
                        iced::widget::row![
                            stroke,
                            opacity_control(state, target, style.opacity)
                        ]
                        .spacing(8)
                        .into(),
                    ),
                }
            }
```

- `preview_annotation`: replace the Task 3 placeholder arm with

```rust
        Annotation::Freehand {
            id,
            kind,
            points,
            mut style,
        } => {
            let mut changed = false;
            if let Some(tx) = state.editor.properties.color.as_ref() {
                if tx.target == PropertyTarget::Annotation(id)
                    && tx.property == ColorProperty::StrokeColor
                {
                    style.color = tx.preview;
                    changed = true;
                }
            }
            if let Some(tx) = state.editor.properties.width.as_ref() {
                if tx.target == PropertyTarget::Annotation(id) {
                    style.width = tx.preview;
                    changed = true;
                }
            }
            if let Some(tx) = state.editor.properties.opacity.as_ref() {
                if tx.target == PropertyTarget::Annotation(id) {
                    style.opacity = tx.preview;
                    changed = true;
                }
            }
            changed.then_some(Annotation::Freehand {
                id,
                kind,
                points,
                style,
            })
        }
```

- [ ] **Step 3: Implement `update.rs` message plumbing**

- Add `PreviewStrokeOpacity(f32)` and `ApplyStrokeOpacity` to the `Message` enum and its manual `PartialEq` (mirror `PreviewStrokeWidth` at `update.rs:251`).
- `clear_property_transactions` (~line 369): add `state.editor.properties.opacity = None;`.
- The Undo property-clear guard (~line 1103): include `|| state.editor.properties.opacity.is_some()` and clear it.
- `PreviewStrokeOpacity` handler (mirror `PreviewStrokeWidth`, ~line 2282). Opacity targets ONLY the Highlighter tool default or a selected Highlighter annotation:

```rust
        Message::PreviewStrokeOpacity(opacity) => {
            use super::properties::{OpacityTransaction, PropertyTarget};
            let Some(target) = super::properties::property_target(state) else {
                return Task::none();
            };
            let original = match target {
                PropertyTarget::FreehandTool(
                    rollshot_image_document::FreehandKind::Highlighter,
                ) => state.annotation_defaults.values.highlighter.opacity,
                PropertyTarget::Annotation(id) => match state.document.image.annotation(id) {
                    Some(Annotation::Freehand {
                        kind: rollshot_image_document::FreehandKind::Highlighter,
                        style,
                        ..
                    }) => style.opacity,
                    _ => return Task::none(),
                },
                _ => return Task::none(),
            };
            let transaction = state
                .editor
                .properties
                .opacity
                .get_or_insert(OpacityTransaction {
                    target,
                    original,
                    preview: original,
                });
            if transaction.target != target {
                *transaction = OpacityTransaction {
                    target,
                    original,
                    preview: original,
                };
            }
            transaction.preview = opacity.clamp(0.1, 1.0);
            state.editor.properties.color = None;
            state.editor.properties.popup = None;
            Task::none()
        }
        Message::ApplyStrokeOpacity => {
            use super::properties::PropertyTarget;
            let Some(transaction) = state.editor.properties.opacity.take() else {
                return Task::none();
            };
            match transaction.target {
                PropertyTarget::FreehandTool(
                    rollshot_image_document::FreehandKind::Highlighter,
                ) => {
                    state.annotation_defaults.values.highlighter.opacity =
                        transaction.preview;
                    persist_annotation_defaults(state);
                }
                PropertyTarget::Annotation(id) => {
                    if let Some(Annotation::Freehand { style, .. }) =
                        state.document.image.annotation(id)
                    {
                        let mut new_style = *style;
                        new_style.opacity = transaction.preview;
                        if let Err(error) = state.document.image.set_stroke_style(id, new_style)
                        {
                            state.message = Some(InlineMessage::Error(error.to_string()));
                        }
                    }
                }
                _ => {}
            }
            Task::none()
        }
```

- Extend the existing stroke color/width paths to freehand:
  - `OpenColorPicker` original lookup: add `(PropertyTarget::FreehandTool(kind), ColorProperty::StrokeColor)` → the matching defaults color, and extend `(PropertyTarget::Annotation(id), ColorProperty::StrokeColor)` to also match `Annotation::Freehand { style, .. } => style.color`.
  - `ApplyColor`: add a `PropertyTarget::FreehandTool(kind)` arm writing `state.annotation_defaults.values.pen.color` / `.highlighter.color` + `persist_annotation_defaults(state)`; extend the `Annotation(id)` `StrokeColor` arm to also match `Annotation::Freehand` (calls `set_stroke_style` exactly like TwoPoint).
  - `PreviewStrokeWidth` original lookup: add `PropertyTarget::FreehandTool(kind)` → defaults width, and extend the `Annotation(id)` arm to match `Annotation::Freehand { style, .. } => style.width`.
  - `ApplyStrokeWidth`: add `PropertyTarget::FreehandTool(kind)` → write the matching defaults width + persist; the `Annotation(id)` arm extends to `Annotation::Freehand` via `set_stroke_style` (preserve the annotation's existing opacity — build `new_style` from the committed style, only changing `width`).

- [ ] **Step 4: Run tests and commit**

Run: `rtk cargo test -p rollshot-app`
Expected: all 6 new tests PASS; existing property tests unaffected.

```bash
rtk git add crates/rollshot-app/src/result_workspace/properties.rs crates/rollshot-app/src/result_workspace/update.rs
rtk git commit -m "feat(annotation): freehand properties with Highlighter-only opacity control"
```

---

### Task 10: Long-image scale test, compatibility sweep, and full verification

**Files:**
- Modify: `crates/rollshot-image-document/src/flatten.rs` (extend the existing 100-annotation stress test at ~line 222 with representative Pen and Highlighter strokes, keeping its history-limit intent — replace some existing entries or extend the mix; do not exceed 100 total if the test asserts the limit)
- Modify: none else expected; fix anything the sweep finds.

- [ ] **Step 1: Extend the 100-annotation test**

Read the existing stress test first. Add Pen and Highlighter strokes to its annotation mix (e.g. every 9th annotation a Pen polyline, every 10th a Highlighter with `highlighter_default()` style, each with 5–20 points spread down the tall image). Preserve the test's original assertions (determinism/duration/history-limit).

Run: `rtk cargo test -p rollshot-image-document -- --nocapture` and note the stress-test duration. If flatten now takes disproportionately long, apply the segment-prefilter optimization noted in Task 2 Step 2 and re-run.

- [ ] **Step 2: Compatibility sweep**

- Run: `rtk cargo test` (whole workspace) — automation, workbench, Timeline, Action Guide, and eval suites must pass unchanged (minus the 3 known pre-existing failures).
- Run: `rtk cargo fmt --all --check`
- Run: `rtk cargo clippy --workspace --all-targets -- -D warnings` — pre-existing `needless_range_loop` warnings in `raster.rs` are known; do not introduce new warnings (the new `stroke_polyline` loops should use the same style as the file's existing loops; if clippy flags them, follow clippy).
- Run: `rtk git diff --check`

- [ ] **Step 3: Spec self-check**

Walk spec §12 (Automated Verification) and confirm each bullet maps to a test added in Tasks 1–10 or a pre-existing suite. Add any missing test inline in the matching module. Pay specific attention to:
- opacity control never appears for Pen/other targets (Task 9 test),
- highlighter opacity config round-trip + forced-1.0 for others (Task 6 tests),
- empty-bbox-corner hit misses (Task 3 hit test),
- self-overlap uniformity and cross-stroke darkening (Tasks 2 and 8),
- Esc/draft cancellation and one-entry-per-gesture history (Task 7 tests).

- [ ] **Step 4: Commit**

```bash
rtk git add -A
rtk git commit -m "test(annotation): freehand long-image scale coverage and verification sweep"
```

---

### Task 11: Registry update and handoff

- [ ] **Step 1: Update the umbrella registry**

In `docs/superpowers/specs/2026-07-12-annotation-editor-umbrella-design.md`, set Slice 4 to `In progress` at implementation start (branch `feat/annotation-freehand-tools`, commit range), and to `Handoff` when implementation + automated verification complete, recording: completed tasks, fresh verification evidence (test counts, fmt/clippy state), remaining work (Linux + macOS native Result Workspace runtime checklists per spec §13), known risks (lyon self-overlap live deviation — observed or not), and the exact next entry point.

- [ ] **Step 2: Verification-before-completion**

Invoke `superpowers:verification-before-completion` before any completion claim; then `superpowers:finishing-a-development-branch` for the integration decision (PR to `main`, mirroring PR #90/#91/#92).

```bash
rtk git add docs/superpowers/specs/2026-07-12-annotation-editor-umbrella-design.md
rtk git commit -m "docs(annotation): record Slice 4 implementation status"
```

---

## Plan Self-Review Notes

- **Spec coverage:** §6 model → Task 3; §7 sampling/simplify/gesture → Tasks 4, 7; §8 rendering/compositing → Task 2 (+8); §9 toolbar/defaults/properties → Tasks 5, 6, 9; §10 output → Task 8; §11 compatibility → Task 3 Step 6 + Task 10; §12 automated verification → Tasks 1–10; §13 runtime checklists → Task 11 handoff (headless environment cannot execute them).
- **Type consistency:** `FreehandKind` (doc crate, re-exported), `Annotation::Freehand { id, kind, points, style }`, `RenderShape::Polyline { points, width, color }`, `add_freehand_with_style` / `set_freehand_points`, `DragState::CreateFreehand { kind, points, style }`, `PropertyTarget::FreehandTool(FreehandKind)`, `OpacityTransaction { target, original, preview }` — used identically across tasks.
- **Known intentional deviations:** freehand minimum-gesture uses the LARGER bbox dimension (a straight stroke must commit), unlike the box tool's two-axis rule; freehand strokes render with round caps while `Line` keeps butt caps (different primitives, both spec'd).
