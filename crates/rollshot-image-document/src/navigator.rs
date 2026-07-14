//! Deterministic Navigator ordering (spec §8.2): image-space top-to-bottom,
//! ties by horizontal position, then stable annotation ID.

use crate::annotation::{Annotation, AnnotationId, TwoPointKind};
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
        Annotation::TwoPoint {
            kind: TwoPointKind::Line,
            ..
        } => "Line".to_string(),
        Annotation::TwoPoint {
            kind: TwoPointKind::Arrow,
            ..
        } => "Arrow".to_string(),
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
        Annotation::Shape { kind, .. } => match kind {
            crate::annotation::ShapeKind::Rectangle => "Rectangle".to_string(),
            crate::annotation::ShapeKind::Ellipse => "Ellipse".to_string(),
        },
        Annotation::Freehand { kind, .. } => match kind {
            crate::annotation::FreehandKind::Pen => "Pen".to_string(),
            crate::annotation::FreehandKind::Highlighter => "Highlighter".to_string(),
        },
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::annotation::{Annotation, AnnotationId, TwoPointKind};
    use crate::geometry::{ImagePoint, ImageRect};
    use crate::style::{NumberStyle, TextStyle};

    fn line() -> Annotation {
        Annotation::two_point(
            AnnotationId(1),
            TwoPointKind::Line,
            ImagePoint::new(10.0, 50.0),
            ImagePoint::new(100.0, 50.0),
        )
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
    fn navigator_labels_two_point_kinds_and_uses_visual_center() {
        let items = navigator_items(&[line(), arrow()]);
        assert_eq!(
            items
                .iter()
                .map(|item| item.label.as_str())
                .collect::<Vec<_>>(),
            ["Line", "Arrow"]
        );
        assert!(items
            .iter()
            .all(|item| item.center.x.is_finite() && item.center.y.is_finite()));
    }

    #[test]
    fn items_sort_by_y_then_x_then_id() {
        let anns = vec![
            Annotation::NumberCallout {
                id: AnnotationId(1),
                number: 1,
                tip: ImagePoint::new(0.0, 500.0),
                bubble: ImagePoint::new(10.0, 500.0),
                style: NumberStyle::default(),
            },
            Annotation::TextNote {
                id: AnnotationId(2),
                position: ImagePoint::new(40.0, 100.0),
                text: "note".to_string(),
                style: TextStyle::default(),
            },
            // Same y as the text note, smaller x — sorts first of the two.
            Annotation::OpaqueRedaction {
                id: AnnotationId(3),
                bounds: ImageRect {
                    x: 5.0,
                    y: 100.0,
                    width: 10.0,
                    height: 10.0,
                },
            },
        ];
        let order: Vec<AnnotationId> = navigator_items(&anns).iter().map(|i| i.id).collect();
        assert_eq!(
            order,
            vec![AnnotationId(3), AnnotationId(2), AnnotationId(1)]
        );
    }

    #[test]
    fn exact_ties_fall_back_to_stable_id() {
        let at = ImagePoint::new(50.0, 50.0);
        let anns = vec![
            Annotation::TextNote {
                id: AnnotationId(9),
                position: at,
                text: "b".into(),
                style: TextStyle::default(),
            },
            Annotation::TextNote {
                id: AnnotationId(4),
                position: at,
                text: "a".into(),
                style: TextStyle::default(),
            },
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
                style: NumberStyle::default(),
            },
            Annotation::TextNote {
                id: AnnotationId(2),
                position: ImagePoint::new(0.0, 10.0),
                text: "first line is quite long and gets truncated\nsecond".to_string(),
                style: TextStyle::default(),
            },
            Annotation::OpaqueRedaction {
                id: AnnotationId(3),
                bounds: ImageRect {
                    x: 0.0,
                    y: 20.0,
                    width: 5.0,
                    height: 5.0,
                },
            },
        ];
        let items = navigator_items(&anns);
        assert_eq!(items[0].label, "7");
        assert_eq!(items[1].label, "first line is quite long…");
        assert!(items[1].label.chars().count() <= 25);
        assert_eq!(items[2].label, "Redaction");
    }

    #[test]
    fn shape_labels_are_rectangle_and_ellipse() {
        let anns = vec![
            Annotation::shape(
                AnnotationId(10),
                crate::annotation::ShapeKind::Rectangle,
                ImageRect {
                    x: 0.0,
                    y: 0.0,
                    width: 10.0,
                    height: 10.0,
                },
            ),
            Annotation::shape(
                AnnotationId(11),
                crate::annotation::ShapeKind::Ellipse,
                ImageRect {
                    x: 20.0,
                    y: 0.0,
                    width: 10.0,
                    height: 10.0,
                },
            ),
        ];
        let items = navigator_items(&anns);
        assert_eq!(items[0].label, "Rectangle");
        assert_eq!(items[1].label, "Ellipse");
    }

    #[test]
    fn shape_anchor_is_logical_top_left() {
        let ann = Annotation::shape(
            AnnotationId(12),
            crate::annotation::ShapeKind::Rectangle,
            ImageRect {
                x: 15.0,
                y: 25.0,
                width: 30.0,
                height: 40.0,
            },
        );
        assert_eq!(ann.anchor(), ImagePoint::new(15.0, 25.0));
    }

    #[test]
    fn freehand_labels_are_pen_and_highlighter() {
        let pen = Annotation::freehand(
            AnnotationId(1),
            crate::annotation::FreehandKind::Pen,
            vec![ImagePoint::new(0.0, 0.0), ImagePoint::new(5.0, 5.0)],
        );
        let hl = Annotation::freehand(
            AnnotationId(2),
            crate::annotation::FreehandKind::Highlighter,
            vec![ImagePoint::new(0.0, 10.0), ImagePoint::new(5.0, 15.0)],
        );
        let items = navigator_items(&[pen, hl]);
        assert_eq!(items[0].label, "Pen");
        assert_eq!(items[1].label, "Highlighter");
    }

    #[test]
    fn freehand_reading_order_and_stable_id_tie_breaking() {
        let a = Annotation::freehand(
            AnnotationId(5),
            crate::annotation::FreehandKind::Pen,
            vec![ImagePoint::new(0.0, 50.0), ImagePoint::new(10.0, 50.0)],
        );
        let b = Annotation::freehand(
            AnnotationId(2),
            crate::annotation::FreehandKind::Pen,
            vec![ImagePoint::new(0.0, 10.0), ImagePoint::new(10.0, 10.0)],
        );
        let items = navigator_items(&[a, b]);
        assert_eq!(items[0].id, AnnotationId(2), "lower y sorts first");
        assert_eq!(items[1].id, AnnotationId(5));
    }
}
