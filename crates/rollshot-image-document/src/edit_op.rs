//! Typed, agent-free document edit operations and their batch outcome.
//! Applied atomically by `ImageDocument::apply_batch` (spec §6.5).

use crate::annotation::{AnnotationId, ShapeKind, TwoPointKind};
use crate::geometry::{ImagePoint, ImageRect, Rgb8};
use crate::style::{NumberStyle, StrokeStyle, TextStyle};

/// A single document mutation. Add* allocate new ids; Update*/Delete reference
/// annotations that exist BEFORE the batch is applied.
#[derive(Debug, Clone, PartialEq)]
pub enum EditOp {
    AddTwoPoint {
        kind: TwoPointKind,
        start: ImagePoint,
        end: ImagePoint,
        style: StrokeStyle,
    },
    AddRedaction {
        bounds: ImageRect,
    },
    AddTextNote {
        position: ImagePoint,
        text: String,
        style: TextStyle,
    },
    AddNumberCallout {
        tip: ImagePoint,
        bubble: ImagePoint,
        style: NumberStyle,
    },
    UpdateRedactionBounds {
        id: AnnotationId,
        bounds: ImageRect,
    },
    UpdateTextPosition {
        id: AnnotationId,
        position: ImagePoint,
    },
    UpdateText {
        id: AnnotationId,
        text: String,
    },
    UpdateNumberPoints {
        id: AnnotationId,
        tip: ImagePoint,
        bubble: ImagePoint,
    },
    UpdateTwoPointPoints {
        id: AnnotationId,
        start: ImagePoint,
        end: ImagePoint,
    },
    UpdateStrokeStyle {
        id: AnnotationId,
        style: StrokeStyle,
    },
    UpdateNumberStyle {
        id: AnnotationId,
        style: NumberStyle,
    },
    UpdateTextStyle {
        id: AnnotationId,
        style: TextStyle,
    },
    SetNextNumber {
        value: u32,
    },
    Delete {
        id: AnnotationId,
    },
    AddShape {
        kind: ShapeKind,
        bounds: ImageRect,
        stroke: StrokeStyle,
        fill: Option<Rgb8>,
    },
    UpdateShapeBounds {
        id: AnnotationId,
        bounds: ImageRect,
    },
    UpdateShapeStyle {
        id: AnnotationId,
        stroke: StrokeStyle,
        fill: Option<Rgb8>,
    },
}

/// Result of a successful `apply_batch`: ids allocated for the Add* ops, in the
/// order those ops appeared in the batch.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct BatchOutcome {
    pub added_ids: Vec<AnnotationId>,
}
