//! The shared render-shape model: the single source of annotation geometry
//! for BOTH flattened output (raster.rs/flatten.rs) and any live overlay
//! renderer.

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::annotation::{Annotation, AnnotationId};
    use crate::geometry::{ImagePoint, ImageRect};
    use crate::style;

    fn number(tip: ImagePoint, bubble: ImagePoint) -> Annotation {
        Annotation::NumberCallout {
            id: AnnotationId(1),
            number: 3,
            tip,
            bubble,
        }
    }

    #[test]
    fn coincident_callout_has_no_leader_triangle() {
        let p = ImagePoint::new(50.0, 50.0);
        let shapes = annotation_shapes(&number(p, p));
        assert!(!shapes
            .iter()
            .any(|s| matches!(s, RenderShape::Triangle { .. })));
        assert!(shapes
            .iter()
            .any(|s| matches!(s, RenderShape::Circle { .. })));
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
        assert!(b.contains(ImagePoint::new(
            100.0 + style::NUMBER_BUBBLE_RADIUS - 1.0,
            100.0
        )));

        let r = Annotation::OpaqueRedaction {
            id: AnnotationId(3),
            bounds: ImageRect { x: 5.0, y: 6.0, width: 7.0, height: 8.0 },
        };
        assert_eq!(
            annotation_bounds(&r),
            ImageRect { x: 5.0, y: 6.0, width: 7.0, height: 8.0 }
        );
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
