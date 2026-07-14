//! The annotation graph. Geometry is stored in full-resolution image
//! coordinates (spec §6); IDs are stable across undo/redo.

use crate::geometry::{ImagePoint, ImageRect, Rgb8};
use crate::style::{NumberStyle, StrokeStyle, TextStyle};

/// Stable annotation identity, restored together with document history.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct AnnotationId(pub u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum TwoPointKind {
    Line,
    Arrow,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
pub enum ShapeKind {
    Rectangle,
    Ellipse,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
pub enum FreehandKind {
    Pen,
    Highlighter,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Annotation {
    TwoPoint {
        id: AnnotationId,
        kind: TwoPointKind,
        start: ImagePoint,
        end: ImagePoint,
        style: StrokeStyle,
    },
    NumberCallout {
        id: AnnotationId,
        number: u32,
        /// The pointed-at location (leader tip).
        tip: ImagePoint,
        /// The number bubble center. Coincident with `tip` for a stamp.
        bubble: ImagePoint,
        style: NumberStyle,
    },
    TextNote {
        id: AnnotationId,
        /// Top-left of the backing plate.
        position: ImagePoint,
        text: String,
        style: TextStyle,
    },
    OpaqueRedaction {
        id: AnnotationId,
        bounds: ImageRect,
    },
    Shape {
        id: AnnotationId,
        kind: ShapeKind,
        bounds: ImageRect,
        stroke: StrokeStyle,
        fill: Option<Rgb8>,
    },
    Freehand {
        id: AnnotationId,
        kind: FreehandKind,
        /// Simplified polyline in full-resolution image coordinates,
        /// stroke start → end.
        points: Vec<ImagePoint>,
        style: StrokeStyle,
    },
}

impl Annotation {
    pub fn two_point(
        id: AnnotationId,
        kind: TwoPointKind,
        start: ImagePoint,
        end: ImagePoint,
    ) -> Self {
        Self::two_point_with_style(id, kind, start, end, StrokeStyle::default())
    }

    pub fn two_point_with_style(
        id: AnnotationId,
        kind: TwoPointKind,
        start: ImagePoint,
        end: ImagePoint,
        style: StrokeStyle,
    ) -> Self {
        Self::TwoPoint {
            id,
            kind,
            start,
            end,
            style,
        }
    }

    pub fn number_callout(
        id: AnnotationId,
        number: u32,
        tip: ImagePoint,
        bubble: ImagePoint,
    ) -> Self {
        Self::number_callout_with_style(id, number, tip, bubble, NumberStyle::default())
    }

    pub fn number_callout_with_style(
        id: AnnotationId,
        number: u32,
        tip: ImagePoint,
        bubble: ImagePoint,
        style: NumberStyle,
    ) -> Self {
        Self::NumberCallout {
            id,
            number,
            tip,
            bubble,
            style,
        }
    }

    pub fn text_note(id: AnnotationId, position: ImagePoint, text: String) -> Self {
        Self::text_note_with_style(id, position, text, TextStyle::default())
    }

    pub fn text_note_with_style(
        id: AnnotationId,
        position: ImagePoint,
        text: String,
        style: TextStyle,
    ) -> Self {
        Self::TextNote {
            id,
            position,
            text,
            style,
        }
    }

    pub fn opaque_redaction(id: AnnotationId, bounds: ImageRect) -> Self {
        Self::OpaqueRedaction { id, bounds }
    }

    pub fn shape(id: AnnotationId, kind: ShapeKind, bounds: ImageRect) -> Self {
        Self::shape_with_style(id, kind, bounds, StrokeStyle::default(), None)
    }

    pub fn shape_with_style(
        id: AnnotationId,
        kind: ShapeKind,
        bounds: ImageRect,
        stroke: StrokeStyle,
        fill: Option<Rgb8>,
    ) -> Self {
        Self::Shape {
            id,
            kind,
            bounds,
            stroke,
            fill,
        }
    }

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

    pub fn id(&self) -> AnnotationId {
        match self {
            Annotation::TwoPoint { id, .. }
            | Annotation::NumberCallout { id, .. }
            | Annotation::TextNote { id, .. }
            | Annotation::OpaqueRedaction { id, .. }
            | Annotation::Shape { id, .. }
            | Annotation::Freehand { id, .. } => *id,
        }
    }

    /// Reading-order anchor used for Navigator ordering (spec §8.2).
    pub fn anchor(&self) -> ImagePoint {
        match self {
            Annotation::TwoPoint { start, end, .. } => {
                ImagePoint::new(start.x.min(end.x), start.y.min(end.y))
            }
            Annotation::NumberCallout { bubble, .. } => *bubble,
            Annotation::TextNote { position, .. } => *position,
            Annotation::OpaqueRedaction { bounds, .. } => ImagePoint::new(bounds.x, bounds.y),
            Annotation::Shape { bounds, .. } => ImagePoint::new(bounds.x, bounds.y),
            Annotation::Freehand { points, .. } => ImagePoint::new(
                points.iter().map(|p| p.x).fold(f32::MAX, f32::min),
                points.iter().map(|p| p.y).fold(f32::MAX, f32::min),
            ),
        }
    }

    pub fn stroke_style(&self) -> Option<StrokeStyle> {
        match self {
            Annotation::TwoPoint { style, .. } => Some(*style),
            Annotation::Shape { stroke, .. } => Some(*stroke),
            Annotation::Freehand { style, .. } => Some(*style),
            _ => None,
        }
    }

    pub fn number_style(&self) -> Option<NumberStyle> {
        match self {
            Annotation::NumberCallout { style, .. } => Some(*style),
            _ => None,
        }
    }

    pub fn text_style(&self) -> Option<TextStyle> {
        match self {
            Annotation::TextNote { style, .. } => Some(*style),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::{ImagePoint, ImageRect};
    use crate::style::{NumberStyle, StrokeStyle, TextStyle};

    #[test]
    fn canonical_two_point_constructor_preserves_kind_and_points() {
        let annotation = Annotation::two_point(
            AnnotationId(7),
            TwoPointKind::Arrow,
            ImagePoint::new(10.0, 20.0),
            ImagePoint::new(80.0, 40.0),
        );
        assert!(matches!(
            annotation,
            Annotation::TwoPoint {
                id: AnnotationId(7),
                kind: TwoPointKind::Arrow,
                start,
                end,
                style,
            } if start == ImagePoint::new(10.0, 20.0)
                && end == ImagePoint::new(80.0, 40.0)
                && style == StrokeStyle::default()
        ));
    }

    #[test]
    fn two_point_anchor_is_endpoint_extent_top_left() {
        let annotation = Annotation::two_point(
            AnnotationId(8),
            TwoPointKind::Line,
            ImagePoint::new(80.0, 20.0),
            ImagePoint::new(10.0, 40.0),
        );
        assert_eq!(annotation.anchor(), ImagePoint::new(10.0, 20.0));
    }

    #[test]
    fn anchor_is_bubble_for_number_position_for_text_topleft_for_redaction() {
        let n = Annotation::NumberCallout {
            id: AnnotationId(1),
            number: 1,
            tip: ImagePoint::new(5.0, 5.0),
            bubble: ImagePoint::new(40.0, 60.0),
            style: NumberStyle::default(),
        };
        assert_eq!(n.anchor(), ImagePoint::new(40.0, 60.0));

        let t = Annotation::TextNote {
            id: AnnotationId(2),
            position: ImagePoint::new(7.0, 8.0),
            text: "hi".to_string(),
            style: TextStyle::default(),
        };
        assert_eq!(t.anchor(), ImagePoint::new(7.0, 8.0));

        let r = Annotation::OpaqueRedaction {
            id: AnnotationId(3),
            bounds: ImageRect {
                x: 1.0,
                y: 2.0,
                width: 3.0,
                height: 4.0,
            },
        };
        assert_eq!(r.anchor(), ImagePoint::new(1.0, 2.0));
    }

    #[test]
    fn id_accessor_returns_each_variant_id() {
        let r = Annotation::OpaqueRedaction {
            id: AnnotationId(9),
            bounds: ImageRect {
                x: 0.0,
                y: 0.0,
                width: 2.0,
                height: 2.0,
            },
        };
        assert_eq!(r.id(), AnnotationId(9));
    }

    #[test]
    fn canonical_constructors_use_default_styles() {
        let n = Annotation::number_callout(
            AnnotationId(1),
            1,
            ImagePoint::new(5.0, 5.0),
            ImagePoint::new(40.0, 60.0),
        );
        assert_eq!(n.number_style(), Some(NumberStyle::default()));

        let t = Annotation::text_note(AnnotationId(2), ImagePoint::new(7.0, 8.0), "hi".to_string());
        assert_eq!(t.text_style(), Some(TextStyle::default()));
    }

    #[test]
    fn two_point_kinds_are_distinct_and_copyable() {
        let line = TwoPointKind::Line;
        let arrow = TwoPointKind::Arrow;
        assert_ne!(line, arrow);
        assert_eq!(line, TwoPointKind::Line);
    }

    #[test]
    fn explicit_style_constructors_store_custom_style() {
        let style = NumberStyle {
            accent: crate::geometry::Rgb8::new(1, 2, 3),
            size: crate::style::NumberSize::Large,
        };
        let n = Annotation::number_callout_with_style(
            AnnotationId(1),
            1,
            ImagePoint::new(5.0, 5.0),
            ImagePoint::new(40.0, 60.0),
            style,
        );
        assert_eq!(n.number_style(), Some(style));

        let ts = TextStyle {
            font_size: crate::style::TextSize::Px32,
            text_color: crate::geometry::Rgb8::new(10, 20, 30),
            background: None,
        };
        let t = Annotation::text_note_with_style(
            AnnotationId(2),
            ImagePoint::new(7.0, 8.0),
            "hi".to_string(),
            ts,
        );
        assert_eq!(t.text_style(), Some(ts));
    }

    #[test]
    fn shape_kinds_are_distinct() {
        assert_ne!(ShapeKind::Rectangle, ShapeKind::Ellipse);
    }

    #[test]
    fn redaction_has_no_style_accessors() {
        let r = Annotation::opaque_redaction(
            AnnotationId(3),
            ImageRect {
                x: 0.0,
                y: 0.0,
                width: 5.0,
                height: 5.0,
            },
        );
        assert_eq!(r.number_style(), None);
        assert_eq!(r.text_style(), None);
    }

    #[test]
    fn freehand_pen_canonical_constructor_uses_default_style() {
        let a = Annotation::freehand(
            AnnotationId(1),
            crate::annotation::FreehandKind::Pen,
            vec![ImagePoint::new(0.0, 0.0), ImagePoint::new(10.0, 5.0)],
        );
        assert_eq!(a.stroke_style(), Some(StrokeStyle::default()));
        assert_eq!(a.id(), AnnotationId(1));
    }

    #[test]
    fn freehand_highlighter_canonical_constructor_uses_highlighter_default() {
        let a = Annotation::freehand(
            AnnotationId(2),
            crate::annotation::FreehandKind::Highlighter,
            vec![ImagePoint::new(0.0, 0.0), ImagePoint::new(10.0, 5.0)],
        );
        assert_eq!(a.stroke_style(), Some(StrokeStyle::highlighter_default()));
    }

    #[test]
    fn freehand_with_style_stores_explicit_style() {
        let style = StrokeStyle {
            width: 8.0,
            ..StrokeStyle::default()
        };
        let a = Annotation::freehand_with_style(
            AnnotationId(3),
            crate::annotation::FreehandKind::Pen,
            vec![ImagePoint::new(0.0, 0.0), ImagePoint::new(10.0, 5.0)],
            style,
        );
        assert_eq!(a.stroke_style(), Some(style));
    }

    #[test]
    fn freehand_anchor_is_minimum_x_minimum_y() {
        let a = Annotation::freehand(
            AnnotationId(4),
            crate::annotation::FreehandKind::Pen,
            vec![
                ImagePoint::new(30.0, 20.0),
                ImagePoint::new(10.0, 40.0),
                ImagePoint::new(50.0, 10.0),
            ],
        );
        assert_eq!(a.anchor(), ImagePoint::new(10.0, 10.0));
    }

    #[test]
    fn freehand_kind_is_copy_eq() {
        let pen = crate::annotation::FreehandKind::Pen;
        let hl = crate::annotation::FreehandKind::Highlighter;
        assert_ne!(pen, hl);
        assert_eq!(pen, crate::annotation::FreehandKind::Pen);
        let copy = hl;
        assert_eq!(hl, copy);
    }
}
