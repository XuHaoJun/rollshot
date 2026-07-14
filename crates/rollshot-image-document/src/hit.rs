//! Image-space hit-testing. Tolerances are passed in by the editor (which
//! converts a fixed screen-space tolerance through its zoom scale).

use crate::annotation::{Annotation, AnnotationId, TwoPointKind};
use crate::box_shape::shape_contains_point;
use crate::geometry::{ImagePoint, ImageRect};
use crate::shapes::text_plate_rect;
use crate::style;
use crate::two_point::{arrowhead_points, point_in_triangle, segment_distance};

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
    StartEndpoint,
    EndEndpoint,
    NumberBubble,
    NumberTip,
    Resize(ResizeHandle),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Hit {
    pub id: AnnotationId,
    pub part: HitPart,
}

/// The 8 resize-handle anchor points of a box-shaped annotation (also used by
/// the editor to draw handles, so hit positions and visuals agree).
pub fn resize_handles(bounds: ImageRect) -> [(ResizeHandle, ImagePoint); 8] {
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

pub fn hit_test_annotation(
    annotation: &Annotation,
    point: ImagePoint,
    tolerance: f32,
) -> Option<HitPart> {
    match annotation {
        Annotation::TwoPoint {
            kind,
            start,
            end,
            style,
            ..
        } => {
            if point.distance(*start) <= tolerance {
                Some(HitPart::StartEndpoint)
            } else if point.distance(*end) <= tolerance {
                Some(HitPart::EndEndpoint)
            } else if segment_distance(point, *start, *end) <= style.width / 2.0 + tolerance
                || (*kind == TwoPointKind::Arrow
                    && point_in_triangle(point, arrowhead_points(*start, *end, style.width)))
            {
                Some(HitPart::Body)
            } else {
                None
            }
        }
        Annotation::NumberCallout {
            tip, bubble, style, ..
        } => {
            let radius = style::NUMBER_BUBBLE_RADIUS * style.size.scale();
            if point.distance(*bubble) <= radius + tolerance {
                Some(HitPart::NumberBubble)
            } else if point.distance(*tip) <= tolerance * 1.6 {
                Some(HitPart::NumberTip)
            } else {
                None
            }
        }
        Annotation::TextNote {
            position,
            text,
            style,
            ..
        } => text_plate_rect(*position, text, *style)
            .expanded(tolerance)
            .contains(point)
            .then_some(HitPart::Body),
        Annotation::OpaqueRedaction { bounds, .. } => {
            for (handle, anchor) in resize_handles(*bounds) {
                if point.distance(anchor) <= tolerance * 1.5 {
                    return Some(HitPart::Resize(handle));
                }
            }
            bounds
                .expanded(tolerance)
                .contains(point)
                .then_some(HitPart::Body)
        }
        Annotation::Shape {
            kind,
            bounds,
            stroke,
            ..
        } => {
            for (handle, anchor) in resize_handles(*bounds) {
                if point.distance(anchor) <= tolerance * 1.5 {
                    return Some(HitPart::Resize(handle));
                }
            }
            shape_contains_point(*kind, *bounds, point, stroke.width / 2.0 + tolerance)
                .then_some(HitPart::Body)
        }
        Annotation::Freehand { points, style, .. } => {
            (crate::freehand::polyline_distance(point, points) <= style.width / 2.0 + tolerance)
                .then_some(HitPart::Body)
        }
    }
}

/// Topmost hit at `point` (later annotations paint on top, so scan reversed).
/// First release: linear scan, no spatial index (spec §13).
pub fn hit_test(annotations: &[Annotation], point: ImagePoint, tolerance: f32) -> Option<Hit> {
    annotations
        .iter()
        .rev()
        .find_map(|a| hit_test_annotation(a, point, tolerance).map(|part| Hit { id: a.id(), part }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::annotation::{Annotation, AnnotationId, TwoPointKind};
    use crate::geometry::{ImagePoint, ImageRect};
    use crate::style;
    use crate::style::StrokeStyle;

    const TOL: f32 = 8.0;

    fn callout() -> Annotation {
        Annotation::NumberCallout {
            id: AnnotationId(1),
            number: 1,
            tip: ImagePoint::new(20.0, 20.0),
            bubble: ImagePoint::new(120.0, 120.0),
            style: style::NumberStyle::default(),
        }
    }

    fn arrow() -> Annotation {
        Annotation::two_point(
            AnnotationId(2),
            TwoPointKind::Arrow,
            ImagePoint::new(10.0, 50.0),
            ImagePoint::new(100.0, 50.0),
        )
    }

    #[test]
    fn arrow_hit_tests_endpoints_shaft_and_triangle_in_priority_order() {
        let annotation = arrow();
        assert_eq!(
            hit_test_annotation(&annotation, ImagePoint::new(10.0, 50.0), 8.0),
            Some(HitPart::StartEndpoint)
        );
        assert_eq!(
            hit_test_annotation(&annotation, ImagePoint::new(100.0, 50.0), 8.0),
            Some(HitPart::EndEndpoint)
        );
        assert_eq!(
            hit_test_annotation(&annotation, ImagePoint::new(50.0, 53.0), 8.0),
            Some(HitPart::Body)
        );
        assert_eq!(
            hit_test_annotation(&annotation, ImagePoint::new(80.0, 58.0), 2.0),
            Some(HitPart::Body)
        );
    }

    #[test]
    fn line_hit_does_not_extend_beyond_finite_segment() {
        let annotation = Annotation::two_point(
            AnnotationId(3),
            TwoPointKind::Line,
            ImagePoint::new(10.0, 50.0),
            ImagePoint::new(100.0, 50.0),
        );
        assert_eq!(
            hit_test_annotation(&annotation, ImagePoint::new(115.0, 50.0), 2.0),
            None
        );
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
        let radius = style::NUMBER_BUBBLE_RADIUS * style::NumberStyle::default().size.scale();
        let just_outside_edge = ImagePoint::new(120.0 + radius + TOL - 1.0, 120.0);
        assert!(hit_test(&anns, just_outside_edge, TOL).is_some());
        let beyond = ImagePoint::new(120.0 + radius + TOL + 2.0, 120.0);
        assert!(hit_test(&anns, beyond, TOL).is_none());
    }

    #[test]
    fn text_note_body_hits_via_plate() {
        let anns = vec![Annotation::TextNote {
            id: AnnotationId(2),
            position: ImagePoint::new(10.0, 10.0),
            text: "hello".to_string(),
            style: style::TextStyle::default(),
        }];
        let hit = hit_test(&anns, ImagePoint::new(14.0, 14.0), TOL).unwrap();
        assert_eq!(hit.part, HitPart::Body);
        assert_eq!(hit.id, AnnotationId(2));
    }

    #[test]
    fn redaction_handles_beat_body_and_corners_resolve() {
        let anns = vec![Annotation::OpaqueRedaction {
            id: AnnotationId(3),
            bounds: ImageRect {
                x: 50.0,
                y: 50.0,
                width: 40.0,
                height: 30.0,
            },
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
                bounds: ImageRect {
                    x: 0.0,
                    y: 0.0,
                    width: 100.0,
                    height: 100.0,
                },
            },
            Annotation::OpaqueRedaction {
                id: AnnotationId(2),
                bounds: ImageRect {
                    x: 25.0,
                    y: 25.0,
                    width: 100.0,
                    height: 100.0,
                },
            },
        ];
        let hit = hit_test(&anns, ImagePoint::new(60.0, 60.0), TOL).unwrap();
        assert_eq!(hit.id, AnnotationId(2), "later annotations draw on top");
    }

    #[test]
    fn shape_rectangle_interior_hits_body() {
        let ann = Annotation::shape(
            AnnotationId(10),
            crate::annotation::ShapeKind::Rectangle,
            ImageRect {
                x: 10.0,
                y: 10.0,
                width: 80.0,
                height: 80.0,
            },
        );
        assert_eq!(
            hit_test_annotation(&ann, ImagePoint::new(50.0, 50.0), TOL),
            Some(HitPart::Body)
        );
    }

    #[test]
    fn shape_ellipse_interior_hits_body() {
        let ann = Annotation::shape(
            AnnotationId(11),
            crate::annotation::ShapeKind::Ellipse,
            ImageRect {
                x: 10.0,
                y: 10.0,
                width: 80.0,
                height: 80.0,
            },
        );
        let center = ImagePoint::new(50.0, 50.0);
        assert_eq!(hit_test_annotation(&ann, center, TOL), Some(HitPart::Body));
    }

    #[test]
    fn shape_ellipse_corner_miss_without_tolerance() {
        let ann = Annotation::shape(
            AnnotationId(12),
            crate::annotation::ShapeKind::Ellipse,
            ImageRect {
                x: 10.0,
                y: 10.0,
                width: 80.0,
                height: 80.0,
            },
        );
        // Point well outside the ellipse (and not on a resize handle)
        assert_eq!(
            hit_test_annotation(&ann, ImagePoint::new(3.0, 3.0), 0.0),
            None
        );
    }

    #[test]
    fn shape_resize_handles_beat_body() {
        let ann = Annotation::shape(
            AnnotationId(13),
            crate::annotation::ShapeKind::Rectangle,
            ImageRect {
                x: 50.0,
                y: 50.0,
                width: 40.0,
                height: 30.0,
            },
        );
        // Top-left corner should hit resize handle, not body
        let hit = hit_test_annotation(&ann, ImagePoint::new(50.0, 50.0), TOL).unwrap();
        assert_eq!(hit, HitPart::Resize(ResizeHandle::TopLeft));
    }

    #[test]
    fn shape_topmost_wins_on_overlap() {
        let anns = vec![
            Annotation::shape(
                AnnotationId(1),
                crate::annotation::ShapeKind::Rectangle,
                ImageRect {
                    x: 0.0,
                    y: 0.0,
                    width: 100.0,
                    height: 100.0,
                },
            ),
            Annotation::shape(
                AnnotationId(2),
                crate::annotation::ShapeKind::Rectangle,
                ImageRect {
                    x: 25.0,
                    y: 25.0,
                    width: 100.0,
                    height: 100.0,
                },
            ),
        ];
        let hit = hit_test(&anns, ImagePoint::new(60.0, 60.0), TOL).unwrap();
        assert_eq!(hit.id, AnnotationId(2));
    }

    #[test]
    fn freehand_hits_near_path_not_in_empty_bbox_corner() {
        let a = Annotation::freehand_with_style(
            AnnotationId(1),
            crate::annotation::FreehandKind::Pen,
            vec![
                ImagePoint::new(0.0, 0.0),
                ImagePoint::new(100.0, 0.0),
                ImagePoint::new(100.0, 100.0),
            ],
            StrokeStyle::default(),
        );
        assert_eq!(
            hit_test_annotation(&a, ImagePoint::new(50.0, 0.0), 2.0),
            Some(HitPart::Body)
        );
        assert_eq!(
            hit_test_annotation(&a, ImagePoint::new(50.0, 3.5), 2.0),
            Some(HitPart::Body)
        );
        assert_eq!(
            hit_test_annotation(&a, ImagePoint::new(20.0, 80.0), 2.0),
            None
        );
    }

    #[test]
    fn freehand_beyond_tolerance_miss() {
        let a = Annotation::freehand_with_style(
            AnnotationId(1),
            crate::annotation::FreehandKind::Pen,
            vec![ImagePoint::new(0.0, 0.0), ImagePoint::new(100.0, 0.0)],
            StrokeStyle::default(), // width 4
        );
        assert_eq!(
            hit_test_annotation(&a, ImagePoint::new(50.0, 5.0), 1.0),
            None
        );
    }

    #[test]
    fn freehand_width_sensitive_hit() {
        let narrow = Annotation::freehand_with_style(
            AnnotationId(1),
            crate::annotation::FreehandKind::Pen,
            vec![ImagePoint::new(0.0, 0.0), ImagePoint::new(100.0, 0.0)],
            StrokeStyle {
                width: 2.0,
                ..StrokeStyle::default()
            },
        );
        let wide = Annotation::freehand_with_style(
            AnnotationId(2),
            crate::annotation::FreehandKind::Pen,
            vec![ImagePoint::new(0.0, 0.0), ImagePoint::new(100.0, 0.0)],
            StrokeStyle {
                width: 20.0,
                ..StrokeStyle::default()
            },
        );
        let pt = ImagePoint::new(50.0, 4.0);
        assert_eq!(hit_test_annotation(&narrow, pt, 1.0), None);
        assert_eq!(hit_test_annotation(&wide, pt, 1.0), Some(HitPart::Body));
    }

    #[test]
    fn freehand_topmost_wins_on_crossing() {
        let anns = vec![
            Annotation::freehand_with_style(
                AnnotationId(1),
                crate::annotation::FreehandKind::Pen,
                vec![ImagePoint::new(0.0, 50.0), ImagePoint::new(100.0, 50.0)],
                StrokeStyle::default(),
            ),
            Annotation::freehand_with_style(
                AnnotationId(2),
                crate::annotation::FreehandKind::Pen,
                vec![ImagePoint::new(50.0, 0.0), ImagePoint::new(50.0, 100.0)],
                StrokeStyle::default(),
            ),
        ];
        let hit = hit_test(&anns, ImagePoint::new(50.0, 50.0), 2.0).unwrap();
        assert_eq!(hit.id, AnnotationId(2));
    }
}
