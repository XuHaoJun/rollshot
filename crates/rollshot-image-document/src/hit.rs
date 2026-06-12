//! Image-space hit-testing. Tolerances are passed in by the editor (which
//! converts a fixed screen-space tolerance through its zoom scale).

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

pub fn hit_test_annotation(
    annotation: &Annotation,
    point: ImagePoint,
    tolerance: f32,
) -> Option<HitPart> {
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
            bounds
                .expanded(tolerance)
                .contains(point)
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
}
