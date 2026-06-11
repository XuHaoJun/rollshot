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
            },
            Annotation::TextNote {
                id: AnnotationId(4),
                position: at,
                text: "a".into(),
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
            },
            Annotation::TextNote {
                id: AnnotationId(2),
                position: ImagePoint::new(0.0, 10.0),
                text: "first line is quite long and gets truncated\nsecond".to_string(),
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
}
