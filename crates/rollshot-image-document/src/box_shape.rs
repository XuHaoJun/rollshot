//! Pure geometry helpers for axis-aligned box shapes (Rectangle, Ellipse).

use crate::annotation::ShapeKind;
use crate::geometry::{ImagePoint, ImageRect};

/// Conservative visual bounds of a box shape including stroke overshoot.
pub fn shape_visual_bounds(bounds: ImageRect, stroke_width: f32) -> ImageRect {
    let half = stroke_width / 2.0;
    bounds.expanded(half)
}

/// Whether `point` lies inside (or within `tolerance` of) the shape.
pub fn shape_contains_point(
    kind: ShapeKind,
    bounds: ImageRect,
    point: ImagePoint,
    tolerance: f32,
) -> bool {
    match kind {
        ShapeKind::Rectangle => {
            let expanded = bounds.expanded(tolerance);
            expanded.contains(point)
        }
        ShapeKind::Ellipse => {
            let cx = bounds.x + bounds.width / 2.0;
            let cy = bounds.y + bounds.height / 2.0;
            let rx = bounds.width / 2.0 + tolerance;
            let ry = bounds.height / 2.0 + tolerance;
            if rx <= 0.0 || ry <= 0.0 {
                return false;
            }
            let dx = (point.x - cx) / rx;
            let dy = (point.y - cy) / ry;
            dx * dx + dy * dy <= 1.0
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::{ImagePoint, ImageRect};

    fn bounds() -> ImageRect {
        ImageRect {
            x: 10.0,
            y: 20.0,
            width: 30.0,
            height: 40.0,
        }
    }

    #[test]
    fn rectangle_contains_interior_point() {
        assert!(shape_contains_point(
            ShapeKind::Rectangle,
            bounds(),
            ImagePoint::new(25.0, 40.0),
            0.0
        ));
    }

    #[test]
    fn ellipse_contains_center() {
        let b = bounds();
        let center = b.center();
        assert!(shape_contains_point(ShapeKind::Ellipse, b, center, 0.0));
    }

    #[test]
    fn ellipse_rejects_corner() {
        let b = bounds();
        assert!(!shape_contains_point(
            ShapeKind::Ellipse,
            b,
            ImagePoint::new(b.x, b.y),
            0.0
        ));
    }

    #[test]
    fn ellipse_corner_hits_with_tolerance() {
        let b = bounds();
        let center_y = b.y + b.height / 2.0;
        assert!(shape_contains_point(
            ShapeKind::Ellipse,
            b,
            ImagePoint::new(b.x, center_y),
            2.0
        ));
    }

    #[test]
    fn visual_bounds_expands_by_half_stroke() {
        let b = bounds();
        let visual = shape_visual_bounds(b, 4.0);
        assert_eq!(visual, b.expanded(2.0));
    }
}
