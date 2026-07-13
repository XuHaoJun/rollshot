//! The shared render-shape model: the single source of annotation geometry
//! for BOTH flattened output (raster.rs/flatten.rs) and any live overlay
//! renderer.

use crate::annotation::{Annotation, ShapeKind, TwoPointKind};
use crate::geometry::{ImagePoint, ImageRect, Rgba8};
use crate::style::{self, NumberStyle, TextStyle};
use crate::text::measure_block;
use crate::two_point::{arrowhead_points, two_point_bounds};

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
    Line {
        start: ImagePoint,
        end: ImagePoint,
        width: f32,
        color: Rgba8,
    },
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
    Box {
        kind: ShapeKind,
        bounds: ImageRect,
        stroke: Rgba8,
        stroke_width: f32,
        fill: Option<Rgba8>,
    },
}

const TEXT_BACKGROUND_ALPHA: u8 = 217;

fn number_radius(style: NumberStyle) -> f32 {
    style::NUMBER_BUBBLE_RADIUS * style.size.scale()
}

/// Font size for a number label, shrunk until it fits the bubble.
pub fn number_label_px(label: &str, style: NumberStyle) -> f32 {
    let radius = number_radius(style);
    let max_width = radius * style::NUMBER_LABEL_MAX_WIDTH_FACTOR;
    let mut px = style::NUMBER_FONT_PX * style.size.scale();
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
pub(crate) fn leader_triangle(
    tip: ImagePoint,
    bubble: ImagePoint,
    style: NumberStyle,
) -> Option<[ImagePoint; 3]> {
    let radius = number_radius(style);
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
    let hw = style::LEADER_HALF_WIDTH * style.size.scale();
    Some([
        tip,
        ImagePoint::new(base.x + normal.0 * hw, base.y + normal.1 * hw),
        ImagePoint::new(base.x - normal.0 * hw, base.y - normal.1 * hw),
    ])
}

/// Backing plate for a text note positioned at `position` (its top-left).
pub fn text_plate_rect(position: ImagePoint, text: &str, style: TextStyle) -> ImageRect {
    let px = style.font_size.pixels();
    let (w, h) = measure_block(text, px, false);
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
        Annotation::TwoPoint {
            kind,
            start,
            end,
            style,
            ..
        } => {
            let alpha = (style.opacity * 255.0).round() as u8;
            let color = style.color.with_alpha(alpha);
            let mut shapes = vec![RenderShape::Line {
                start: *start,
                end: *end,
                width: style.width,
                color,
            }];
            if *kind == TwoPointKind::Arrow && start.distance(*end) > 0.0 {
                shapes.push(RenderShape::Triangle {
                    points: arrowhead_points(*start, *end, style.width),
                    color,
                });
            }
            shapes
        }
        Annotation::NumberCallout {
            number,
            tip,
            bubble,
            style,
            ..
        } => {
            let radius = number_radius(*style);
            let outline_width = style::NUMBER_BUBBLE_OUTLINE_WIDTH * style.size.scale();
            let mut shapes = Vec::with_capacity(3);
            if let Some(points) = leader_triangle(*tip, *bubble, *style) {
                shapes.push(RenderShape::Triangle {
                    points,
                    color: style.accent.opaque(),
                });
            }
            shapes.push(RenderShape::Circle {
                center: *bubble,
                radius,
                fill: style.accent.opaque(),
                outline_width,
                outline: style::WHITE,
            });
            let label = number.to_string();
            let px = number_label_px(&label, *style);
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
        Annotation::TextNote {
            position,
            text,
            style,
            ..
        } => {
            let pad = style::TEXT_NOTE_PLATE_PADDING;
            let mut shapes = Vec::with_capacity(2);
            if let Some(bg) = style.background {
                shapes.push(RenderShape::Rect {
                    rect: text_plate_rect(*position, text, *style),
                    color: bg.with_alpha(TEXT_BACKGROUND_ALPHA),
                });
            }
            let label_x = position.x + pad;
            let label_y = position.y + pad;
            shapes.push(RenderShape::Label {
                anchor: ImagePoint::new(label_x, label_y),
                anchor_kind: TextAnchor::TopLeft,
                content: text.clone(),
                px: style.font_size.pixels(),
                bold: false,
                color: style.text_color.opaque(),
            });
            shapes
        }
        Annotation::OpaqueRedaction { bounds, .. } => vec![RenderShape::Rect {
            rect: *bounds,
            color: style::REDACTION_FILL,
        }],
        Annotation::Shape {
            kind,
            bounds,
            stroke,
            fill,
            ..
        } => {
            let alpha = (stroke.opacity * 255.0).round() as u8;
            let stroke_color = stroke.color.with_alpha(alpha);
            let fill_color = fill.map(|c| c.opaque());
            vec![RenderShape::Box {
                kind: *kind,
                bounds: *bounds,
                stroke: stroke_color,
                stroke_width: stroke.width,
                fill: fill_color,
            }]
        }
    }
}

/// Conservative image-space bounds of an annotation's visuals — used for
/// viewport culling and Navigator jump targets.
pub fn annotation_bounds(annotation: &Annotation) -> ImageRect {
    match annotation {
        Annotation::TwoPoint {
            kind,
            start,
            end,
            style,
            ..
        } => two_point_bounds(*kind, *start, *end, style.width),
        Annotation::NumberCallout {
            tip, bubble, style, ..
        } => {
            let radius = number_radius(*style);
            let outline_width = style::NUMBER_BUBBLE_OUTLINE_WIDTH * style.size.scale();
            let r = radius + outline_width;
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
            ImageRect {
                x: x0,
                y: y0,
                width: x1 - x0,
                height: y1 - y0,
            }
        }
        Annotation::TextNote {
            position,
            text,
            style,
            ..
        } => text_plate_rect(*position, text, *style),
        Annotation::OpaqueRedaction { bounds, .. } => *bounds,
        Annotation::Shape { bounds, stroke, .. } => {
            crate::box_shape::shape_visual_bounds(*bounds, stroke.width)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::annotation::{Annotation, AnnotationId, TwoPointKind};
    use crate::geometry::{ImagePoint, ImageRect, Rgb8};
    use crate::style::{self, NumberSize, NumberStyle, StrokeStyle, TextSize, TextStyle};

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
    fn arrow_lowers_to_shaft_then_existing_triangle() {
        let annotation = arrow();
        let shapes = annotation_shapes(&annotation);
        assert!(matches!(shapes[0], RenderShape::Line { .. }));
        assert!(matches!(shapes[1], RenderShape::Triangle { .. }));
    }

    #[test]
    fn coincident_arrow_draft_lowers_without_an_arrowhead() {
        let point = ImagePoint::new(25.0, 30.0);
        let annotation =
            Annotation::two_point(AnnotationId(u64::MAX), TwoPointKind::Arrow, point, point);

        assert_eq!(
            annotation_shapes(&annotation),
            vec![RenderShape::Line {
                start: point,
                end: point,
                width: 4.0,
                color: Rgb8::new(0xE5, 0x48, 0x4D).opaque(),
            }]
        );
    }

    #[test]
    fn line_lowers_to_one_shaft_with_reviewed_style() {
        let shapes = annotation_shapes(&line());
        assert_eq!(shapes.len(), 1);
        assert!(matches!(
            shapes[0],
            RenderShape::Line {
                start: ImagePoint { x: 10.0, y: 50.0 },
                end: ImagePoint { x: 100.0, y: 50.0 },
                width: 4.0,
                color: Rgba8 {
                    r: 0xE5,
                    g: 0x48,
                    b: 0x4D,
                    a: 0xFF
                },
            }
        ));
    }

    fn number(tip: ImagePoint, bubble: ImagePoint) -> Annotation {
        Annotation::NumberCallout {
            id: AnnotationId(1),
            number: 3,
            tip,
            bubble,
            style: NumberStyle::default(),
        }
    }

    fn number_with_style(style: NumberStyle) -> Annotation {
        Annotation::NumberCallout {
            id: AnnotationId(1),
            number: 3,
            tip: ImagePoint::new(10.0, 10.0),
            bubble: ImagePoint::new(100.0, 100.0),
            style,
        }
    }

    fn text_with_style(style: TextStyle) -> Annotation {
        Annotation::TextNote {
            id: AnnotationId(2),
            position: ImagePoint::new(20.0, 30.0),
            text: "hello".to_string(),
            style,
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
        let style = TextStyle::default();
        let plate = text_plate_rect(pos, "hello", style);
        let (w, h) = crate::text::measure_block("hello", style.font_size.pixels(), false);
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
            style: TextStyle::default(),
        };
        let shapes = annotation_shapes(&note);
        assert!(matches!(shapes[0], RenderShape::Rect { .. }));
        match &shapes[1] {
            RenderShape::Label {
                anchor,
                anchor_kind,
                bold,
                ..
            } => {
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
        let radius = number_radius(NumberStyle::default());
        assert!(b.contains(ImagePoint::new(100.0 + radius - 1.0, 100.0)));

        let r = Annotation::OpaqueRedaction {
            id: AnnotationId(3),
            bounds: ImageRect {
                x: 5.0,
                y: 6.0,
                width: 7.0,
                height: 8.0,
            },
        };
        assert_eq!(
            annotation_bounds(&r),
            ImageRect {
                x: 5.0,
                y: 6.0,
                width: 7.0,
                height: 8.0
            }
        );
    }

    #[test]
    fn long_number_labels_shrink_to_fit() {
        let style = NumberStyle::default();
        let small = number_label_px("3", style);
        let large = number_label_px("888", style);
        assert_eq!(small, style::NUMBER_FONT_PX);
        assert!(
            large < small,
            "3-digit labels shrink to stay inside the bubble"
        );
        assert!(large >= style::NUMBER_FONT_MIN_PX);
    }

    #[test]
    fn number_size_scales_render_shapes_and_bounds() {
        let small = number_with_style(NumberStyle {
            size: NumberSize::Small,
            ..Default::default()
        });
        let large = number_with_style(NumberStyle {
            size: NumberSize::Large,
            ..Default::default()
        });
        assert!(
            annotation_bounds(&large).width > annotation_bounds(&small).width,
            "large number callout must have wider bounds than small"
        );
        assert!(annotation_shapes(&large).iter().any(
            |s| matches!(s, RenderShape::Circle { fill, .. } if *fill == NumberStyle::default().accent.opaque())
        ));
    }

    #[test]
    fn text_style_controls_font_color_and_optional_fixed_alpha_plate() {
        let style = TextStyle {
            font_size: TextSize::Px32,
            text_color: Rgb8::new(1, 2, 3),
            background: Some(Rgb8::new(4, 5, 6)),
        };
        let shapes = annotation_shapes(&text_with_style(style));
        assert!(
            matches!(shapes[0], RenderShape::Rect { color, .. } if color == Rgba8::new(4, 5, 6, 217)),
            "plate color must use style background with 85% alpha"
        );
        assert!(
            matches!(shapes[1], RenderShape::Label { px, color, .. } if (px - 32.0).abs() < f32::EPSILON && color == Rgba8::new(1, 2, 3, 255)),
            "label must use style font_size and text_color"
        );
    }

    #[test]
    fn text_without_background_emits_only_the_label() {
        let style = TextStyle {
            background: None,
            ..Default::default()
        };
        assert_eq!(
            annotation_shapes(&text_with_style(style)).len(),
            1,
            "no background → only the label shape"
        );
    }

    // --- Shape tests ---

    #[test]
    fn shape_lowers_to_box_with_correct_kind_and_bounds() {
        let ann = Annotation::shape(
            AnnotationId(10),
            crate::annotation::ShapeKind::Rectangle,
            ImageRect {
                x: 10.0,
                y: 20.0,
                width: 30.0,
                height: 40.0,
            },
        );
        let shapes = annotation_shapes(&ann);
        assert_eq!(shapes.len(), 1);
        match &shapes[0] {
            RenderShape::Box {
                kind,
                bounds,
                stroke,
                stroke_width,
                fill,
            } => {
                assert_eq!(*kind, crate::annotation::ShapeKind::Rectangle);
                assert_eq!(
                    *bounds,
                    ImageRect {
                        x: 10.0,
                        y: 20.0,
                        width: 30.0,
                        height: 40.0
                    }
                );
                assert_eq!(*stroke, Rgb8::new(0xE5, 0x48, 0x4D).with_alpha(255));
                assert_eq!(*stroke_width, 4.0);
                assert_eq!(*fill, None);
            }
            _ => panic!("expected Box"),
        }
    }

    #[test]
    fn shape_with_fill_lowers_to_box_with_fill() {
        let ann = Annotation::shape_with_style(
            AnnotationId(11),
            crate::annotation::ShapeKind::Ellipse,
            ImageRect {
                x: 5.0,
                y: 5.0,
                width: 20.0,
                height: 20.0,
            },
            StrokeStyle {
                color: Rgb8::new(10, 20, 30),
                width: 2.0,
                opacity: 0.5,
            },
            Some(Rgb8::new(100, 200, 50)),
        );
        let shapes = annotation_shapes(&ann);
        match &shapes[0] {
            RenderShape::Box {
                kind, stroke, fill, ..
            } => {
                assert_eq!(*kind, crate::annotation::ShapeKind::Ellipse);
                assert_eq!(*stroke, Rgb8::new(10, 20, 30).with_alpha(128));
                assert_eq!(*fill, Some(Rgb8::new(100, 200, 50).opaque()));
            }
            _ => panic!("expected Box"),
        }
    }

    #[test]
    fn shape_bounds_expands_by_half_stroke_width() {
        let ann = Annotation::shape(
            AnnotationId(12),
            crate::annotation::ShapeKind::Rectangle,
            ImageRect {
                x: 10.0,
                y: 20.0,
                width: 30.0,
                height: 40.0,
            },
        );
        let b = annotation_bounds(&ann);
        assert_eq!(
            b,
            ImageRect {
                x: 8.0,
                y: 18.0,
                width: 34.0,
                height: 44.0,
            }
        );
    }
}
