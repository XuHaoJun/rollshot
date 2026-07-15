//! The non-destructive image document: immutable source, annotation graph,
//! number sequence, and snapshot-based history (spec §6, §10).

use std::collections::VecDeque;
use std::sync::Arc;

use image::RgbaImage;

use crate::annotation::{Annotation, AnnotationId, ShapeKind, TwoPointKind};
use crate::edit_op::{BatchOutcome, EditOp};
use crate::geometry::{ImagePoint, ImageRect, Rgb8};
use crate::hit::Hit;
use crate::navigator::NavigatorItem;
use crate::style::StrokeStyle;

/// Maximum undo entries (spec §10).
pub const HISTORY_LIMIT: usize = 100;

fn ensure_point_finite(p: &ImagePoint) -> Result<(), EditError> {
    if p.is_finite() {
        Ok(())
    } else {
        Err(EditError::NonFiniteCoordinate)
    }
}

fn ensure_rect_finite(r: &ImageRect) -> Result<(), EditError> {
    if r.is_finite() {
        Ok(())
    } else {
        Err(EditError::NonFiniteCoordinate)
    }
}

fn validate_stroke_style(style: StrokeStyle) -> Result<(), EditError> {
    if !style.width.is_finite() || style.width <= 0.0 {
        return Err(EditError::InvalidStrokeWidth);
    }
    if !style.opacity.is_finite() || !(0.0..=1.0).contains(&style.opacity) {
        return Err(EditError::InvalidOpacity);
    }
    Ok(())
}

fn clamp_two_point(
    start: ImagePoint,
    end: ImagePoint,
    width: u32,
    height: u32,
) -> Result<(ImagePoint, ImagePoint), EditError> {
    ensure_point_finite(&start)?;
    ensure_point_finite(&end)?;
    let start = start.clamp_to(width, height);
    let end = end.clamp_to(width, height);
    if start == end {
        return Err(EditError::CoincidentPoints);
    }
    Ok((start, end))
}

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
    let clamped: Vec<ImagePoint> = points
        .into_iter()
        .map(|p| p.clamp_to(width, height))
        .collect();
    if clamped.iter().all(|p| *p == clamped[0]) {
        return Err(EditError::InvalidFreehandPath);
    }
    Ok(clamped)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum EditError {
    #[error("two-point annotations require distinct endpoints")]
    CoincidentPoints,
    #[error("stroke width must be finite and greater than zero")]
    InvalidStrokeWidth,
    #[error("stroke opacity must be finite and between zero and one")]
    InvalidOpacity,
    #[error("text notes must contain non-whitespace text")]
    EmptyText,
    #[error("redactions must cover at least one pixel")]
    ZeroArea,
    #[error("annotation does not exist")]
    UnknownAnnotation,
    #[error("operation does not apply to this annotation kind")]
    WrongKind,
    #[error("coordinates must be finite")]
    NonFiniteCoordinate,
    #[error("next number must be at least 1")]
    InvalidNextNumber,
    #[error("shape bounds must be finite, positive, and cover at least one pixel")]
    InvalidShapeBounds,
    #[error("freehand strokes require at least two distinct finite points")]
    InvalidFreehandPath,
    #[error("pixelate bounds must be finite, positive, and cover at least one pixel")]
    InvalidPixelateBounds,
    #[error("pixelate block size {0} is outside the allowed range")]
    InvalidPixelateBlockSize(u32),
}

/// One restorable history state (mark-shot pattern: graph + counters).
#[derive(Debug, Clone)]
struct Snapshot {
    annotations: Vec<Annotation>,
    next_number: u32,
    next_id: u64,
    state_id: u64,
}

pub struct ImageDocument {
    source: Arc<RgbaImage>,
    annotations: Vec<Annotation>,
    next_number: u32,
    next_id: u64,
    /// Identity of the current document state; restored by undo/redo so the
    /// editor can compare against a saved marker (dirty tracking).
    state_id: u64,
    next_state_id: u64,
    undo_stack: VecDeque<Snapshot>,
    redo_stack: Vec<Snapshot>,
}

impl ImageDocument {
    pub fn new(source: RgbaImage) -> Self {
        Self {
            source: Arc::new(source),
            annotations: Vec::new(),
            next_number: 1,
            next_id: 1,
            state_id: 0,
            next_state_id: 0,
            undo_stack: VecDeque::new(),
            redo_stack: Vec::new(),
        }
    }

    pub fn source(&self) -> &RgbaImage {
        &self.source
    }

    pub fn shared_source(&self) -> Arc<RgbaImage> {
        Arc::clone(&self.source)
    }

    pub fn annotations(&self) -> &[Annotation] {
        &self.annotations
    }

    pub fn annotation(&self, id: AnnotationId) -> Option<&Annotation> {
        self.annotations.iter().find(|a| a.id() == id)
    }

    pub fn next_number(&self) -> u32 {
        self.next_number
    }

    pub fn state_id(&self) -> u64 {
        self.state_id
    }

    pub fn navigator_items(&self) -> Vec<NavigatorItem> {
        crate::navigator::navigator_items(&self.annotations)
    }

    pub fn hit_test(&self, point: ImagePoint, tolerance: f32) -> Option<Hit> {
        crate::hit::hit_test(&self.annotations, point, tolerance)
    }

    /// Render the annotated full-resolution output. Infallible and
    /// non-mutating; called only for explicit Copy/Save actions (spec §11.2).
    pub fn flatten(&self) -> RgbaImage {
        crate::flatten::flatten_onto(&self.source, &self.annotations)
    }

    pub fn can_undo(&self) -> bool {
        !self.undo_stack.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo_stack.is_empty()
    }

    fn snapshot(&self) -> Snapshot {
        Snapshot {
            annotations: self.annotations.clone(),
            next_number: self.next_number,
            next_id: self.next_id,
            state_id: self.state_id,
        }
    }

    /// Record `before` as an undo entry and stamp a fresh state id.
    /// Called exactly once per completed semantic edit (spec §10).
    fn commit(&mut self, before: Snapshot) {
        self.undo_stack.push_back(before);
        if self.undo_stack.len() > HISTORY_LIMIT {
            self.undo_stack.pop_front();
        }
        self.redo_stack.clear();
        self.next_state_id += 1;
        self.state_id = self.next_state_id;
    }

    fn restore(&mut self, snapshot: Snapshot) {
        self.annotations = snapshot.annotations;
        self.next_number = snapshot.next_number;
        self.next_id = snapshot.next_id;
        self.state_id = snapshot.state_id;
    }

    /// Returns `false` when there is nothing to undo.
    pub fn undo(&mut self) -> bool {
        let Some(previous) = self.undo_stack.pop_back() else {
            return false;
        };
        self.redo_stack.push(self.snapshot());
        self.restore(previous);
        true
    }

    /// Returns `false` when there is nothing to redo.
    pub fn redo(&mut self) -> bool {
        let Some(next) = self.redo_stack.pop() else {
            return false;
        };
        self.undo_stack.push_back(self.snapshot());
        self.restore(next);
        true
    }

    fn allocate_id(&mut self) -> AnnotationId {
        let id = AnnotationId(self.next_id);
        self.next_id += 1;
        id
    }

    pub fn add_two_point(
        &mut self,
        kind: TwoPointKind,
        start: ImagePoint,
        end: ImagePoint,
    ) -> Result<AnnotationId, EditError> {
        self.add_two_point_with_style(kind, start, end, StrokeStyle::default())
    }

    pub fn add_two_point_with_style(
        &mut self,
        kind: TwoPointKind,
        start: ImagePoint,
        end: ImagePoint,
        style: StrokeStyle,
    ) -> Result<AnnotationId, EditError> {
        let outcome = self.apply_batch(vec![EditOp::AddTwoPoint {
            kind,
            start,
            end,
            style,
        }])?;
        Ok(outcome.added_ids[0])
    }

    pub fn set_two_point_points(
        &mut self,
        id: AnnotationId,
        start: ImagePoint,
        end: ImagePoint,
    ) -> Result<(), EditError> {
        self.apply_batch(vec![EditOp::UpdateTwoPointPoints { id, start, end }])?;
        Ok(())
    }

    pub fn set_stroke_style(
        &mut self,
        id: AnnotationId,
        style: StrokeStyle,
    ) -> Result<(), EditError> {
        self.apply_batch(vec![EditOp::UpdateStrokeStyle { id, style }])?;
        Ok(())
    }

    pub fn add_number_callout(&mut self, tip: ImagePoint, bubble: ImagePoint) -> AnnotationId {
        self.add_number_callout_with_style(tip, bubble, crate::style::NumberStyle::default())
    }

    pub fn add_number_callout_with_style(
        &mut self,
        tip: ImagePoint,
        bubble: ImagePoint,
        style: crate::style::NumberStyle,
    ) -> AnnotationId {
        let before = self.snapshot();
        let (w, h) = self.source.dimensions();
        let id = self.allocate_id();
        let number = self.next_number;
        self.next_number += 1;
        self.annotations.push(Annotation::NumberCallout {
            id,
            number,
            tip: tip.clamp_to(w, h),
            bubble: bubble.clamp_to(w, h),
            style,
        });
        self.commit(before);
        id
    }

    pub fn add_text_note(
        &mut self,
        position: ImagePoint,
        text: String,
    ) -> Result<AnnotationId, EditError> {
        self.add_text_note_with_style(position, text, crate::style::TextStyle::default())
    }

    pub fn add_text_note_with_style(
        &mut self,
        position: ImagePoint,
        text: String,
        style: crate::style::TextStyle,
    ) -> Result<AnnotationId, EditError> {
        if text.trim().is_empty() {
            return Err(EditError::EmptyText);
        }
        let before = self.snapshot();
        let (w, h) = self.source.dimensions();
        let id = self.allocate_id();
        self.annotations.push(Annotation::TextNote {
            id,
            position: position.clamp_to(w, h),
            text,
            style,
        });
        self.commit(before);
        Ok(id)
    }

    pub fn add_redaction(&mut self, bounds: ImageRect) -> Result<AnnotationId, EditError> {
        let (w, h) = self.source.dimensions();
        let clamped = bounds.clamp_to(w, h);
        if clamped.is_empty() {
            return Err(EditError::ZeroArea);
        }
        let before = self.snapshot();
        let id = self.allocate_id();
        self.annotations.push(Annotation::OpaqueRedaction {
            id,
            bounds: clamped,
        });
        self.commit(before);
        Ok(id)
    }

    fn annotation_index(&self, id: AnnotationId) -> Result<usize, EditError> {
        self.annotations
            .iter()
            .position(|a| a.id() == id)
            .ok_or(EditError::UnknownAnnotation)
    }

    pub fn set_number_points(
        &mut self,
        id: AnnotationId,
        tip: ImagePoint,
        bubble: ImagePoint,
    ) -> Result<(), EditError> {
        let (w, h) = self.source.dimensions();
        let (tip, bubble) = (tip.clamp_to(w, h), bubble.clamp_to(w, h));
        let index = self.annotation_index(id)?;
        let before = self.snapshot();
        match &mut self.annotations[index] {
            Annotation::NumberCallout {
                tip: t, bubble: b, ..
            } => {
                if *t == tip && *b == bubble {
                    return Ok(());
                }
                *t = tip;
                *b = bubble;
            }
            _ => return Err(EditError::WrongKind),
        }
        self.commit(before);
        Ok(())
    }

    pub fn set_text_position(
        &mut self,
        id: AnnotationId,
        position: ImagePoint,
    ) -> Result<(), EditError> {
        let (w, h) = self.source.dimensions();
        let position = position.clamp_to(w, h);
        let index = self.annotation_index(id)?;
        let before = self.snapshot();
        match &mut self.annotations[index] {
            Annotation::TextNote { position: p, .. } => {
                if *p == position {
                    return Ok(());
                }
                *p = position;
            }
            _ => return Err(EditError::WrongKind),
        }
        self.commit(before);
        Ok(())
    }

    pub fn set_text(&mut self, id: AnnotationId, text: String) -> Result<(), EditError> {
        if text.trim().is_empty() {
            return Err(EditError::EmptyText);
        }
        let index = self.annotation_index(id)?;
        let before = self.snapshot();
        match &mut self.annotations[index] {
            Annotation::TextNote { text: t, .. } => {
                if *t == text {
                    return Ok(());
                }
                *t = text;
            }
            _ => return Err(EditError::WrongKind),
        }
        self.commit(before);
        Ok(())
    }

    pub fn set_redaction_bounds(
        &mut self,
        id: AnnotationId,
        bounds: ImageRect,
    ) -> Result<(), EditError> {
        let (w, h) = self.source.dimensions();
        let clamped = bounds.clamp_to(w, h);
        if clamped.is_empty() {
            return Err(EditError::ZeroArea);
        }
        let index = self.annotation_index(id)?;
        let before = self.snapshot();
        match &mut self.annotations[index] {
            Annotation::OpaqueRedaction { bounds: b, .. } => {
                if *b == clamped {
                    return Ok(());
                }
                *b = clamped;
            }
            _ => return Err(EditError::WrongKind),
        }
        self.commit(before);
        Ok(())
    }

    /// Delete an annotation. Deleting a Number Callout compactly renumbers
    /// the remaining callouts preserving relative order; the deletion and its
    /// renumbering form ONE history entry (spec §9.2, decision D1).
    pub fn delete_annotation(&mut self, id: AnnotationId) -> Result<(), EditError> {
        let index = self.annotation_index(id)?;
        let before = self.snapshot();
        let removed = self.annotations.remove(index);
        if matches!(removed, Annotation::NumberCallout { .. }) {
            self.renumber_compactly();
        }
        self.commit(before);
        Ok(())
    }

    pub fn set_number_style(
        &mut self,
        id: AnnotationId,
        style: crate::style::NumberStyle,
    ) -> Result<(), EditError> {
        let index = self.annotation_index(id)?;
        match &self.annotations[index] {
            Annotation::NumberCallout { style: s, .. } if *s == style => return Ok(()),
            Annotation::NumberCallout { .. } => {}
            _ => return Err(EditError::WrongKind),
        }
        let before = self.snapshot();
        if let Annotation::NumberCallout { style: s, .. } = &mut self.annotations[index] {
            *s = style;
        }
        self.commit(before);
        Ok(())
    }

    pub fn set_text_style(
        &mut self,
        id: AnnotationId,
        style: crate::style::TextStyle,
    ) -> Result<(), EditError> {
        let index = self.annotation_index(id)?;
        match &self.annotations[index] {
            Annotation::TextNote { style: s, .. } if *s == style => return Ok(()),
            Annotation::TextNote { .. } => {}
            _ => return Err(EditError::WrongKind),
        }
        let before = self.snapshot();
        if let Annotation::TextNote { style: s, .. } = &mut self.annotations[index] {
            *s = style;
        }
        self.commit(before);
        Ok(())
    }

    pub fn set_next_number(&mut self, value: u32) -> Result<(), EditError> {
        if value == 0 {
            return Err(EditError::InvalidNextNumber);
        }
        if self.next_number == value {
            return Ok(());
        }
        let before = self.snapshot();
        self.next_number = value;
        self.commit(before);
        Ok(())
    }

    pub fn add_shape(
        &mut self,
        kind: ShapeKind,
        bounds: ImageRect,
    ) -> Result<AnnotationId, EditError> {
        self.add_shape_with_style(kind, bounds, StrokeStyle::default(), None)
    }

    pub fn add_shape_with_style(
        &mut self,
        kind: ShapeKind,
        bounds: ImageRect,
        stroke: StrokeStyle,
        fill: Option<Rgb8>,
    ) -> Result<AnnotationId, EditError> {
        let outcome = self.apply_batch(vec![EditOp::AddShape {
            kind,
            bounds,
            stroke,
            fill,
        }])?;
        Ok(outcome.added_ids[0])
    }

    pub fn set_shape_bounds(
        &mut self,
        id: AnnotationId,
        bounds: ImageRect,
    ) -> Result<(), EditError> {
        self.apply_batch(vec![EditOp::UpdateShapeBounds { id, bounds }])?;
        Ok(())
    }

    pub fn set_shape_style(
        &mut self,
        id: AnnotationId,
        stroke: StrokeStyle,
        fill: Option<Rgb8>,
    ) -> Result<(), EditError> {
        self.apply_batch(vec![EditOp::UpdateShapeStyle { id, stroke, fill }])?;
        Ok(())
    }

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

    pub fn add_pixelate(
        &mut self,
        bounds: ImageRect,
        block_size: u32,
    ) -> Result<AnnotationId, EditError> {
        let outcome = self.apply_batch(vec![EditOp::AddPixelate { bounds, block_size }])?;
        Ok(outcome.added_ids[0])
    }

    pub fn set_pixelate_bounds(
        &mut self,
        id: AnnotationId,
        bounds: ImageRect,
    ) -> Result<(), EditError> {
        self.apply_batch(vec![EditOp::UpdatePixelateBounds { id, bounds }])?;
        Ok(())
    }

    pub fn set_pixelate_block_size(
        &mut self,
        id: AnnotationId,
        block_size: u32,
    ) -> Result<(), EditError> {
        self.apply_batch(vec![EditOp::UpdatePixelateBlockSize { id, block_size }])?;
        Ok(())
    }

    /// Apply many operations as ONE history entry (spec §6.5). Atomic: if any
    /// op is invalid the whole batch is rolled back — no mutation, no commit,
    /// no `state_id` change. Update*/Delete reference annotations existing
    /// before the batch. An empty batch is a no-op with no history entry.
    ///
    /// ```text
    /// ops.is_empty()? --yes--> Ok(BatchOutcome::default)   (no snapshot/commit)
    ///        | no
    ///        v
    ///   snapshot(before) once
    ///        v
    ///   for op in ops: apply_one(op)
    ///        |                       \
    ///     all Ok                    first Err(e)
    ///        v                            v
    ///   (callout deleted? renumber)  restore(before)   (no commit; state_id unchanged)
    ///        v                            v
    ///   commit(before) once            Err(e)
    ///        v
    ///   Ok(BatchOutcome { added_ids })
    /// ```
    pub fn apply_batch(&mut self, ops: Vec<EditOp>) -> Result<BatchOutcome, EditError> {
        if ops.is_empty() {
            return Ok(BatchOutcome::default());
        }
        for op in &ops {
            let referenced_id = match op {
                EditOp::UpdateRedactionBounds { id, .. }
                | EditOp::UpdateTextPosition { id, .. }
                | EditOp::UpdateText { id, .. }
                | EditOp::UpdateNumberPoints { id, .. }
                | EditOp::UpdateNumberStyle { id, .. }
                | EditOp::UpdateTextStyle { id, .. }
                | EditOp::UpdateTwoPointPoints { id, .. }
                | EditOp::UpdateStrokeStyle { id, .. }
                | EditOp::UpdateShapeBounds { id, .. }
                | EditOp::UpdateShapeStyle { id, .. }
                | EditOp::UpdateFreehandPoints { id, .. }
                | EditOp::UpdatePixelateBounds { id, .. }
                | EditOp::UpdatePixelateBlockSize { id, .. }
                | EditOp::Delete { id } => Some(*id),
                EditOp::AddTwoPoint { .. }
                | EditOp::AddRedaction { .. }
                | EditOp::AddTextNote { .. }
                | EditOp::AddNumberCallout { .. }
                | EditOp::AddShape { .. }
                | EditOp::AddFreehand { .. }
                | EditOp::AddPixelate { .. }
                | EditOp::SetNextNumber { .. } => None,
            };
            if referenced_id.is_some_and(|id| self.annotation(id).is_none()) {
                return Err(EditError::UnknownAnnotation);
            }
        }
        let (w, h) = self.source.dimensions();
        let before = self.snapshot();
        let mut added_ids = Vec::new();
        let mut deleted_callout = false;
        let mut failure: Option<EditError> = None;
        for op in ops {
            if let Err(e) = self.apply_one(op, w, h, &mut added_ids, &mut deleted_callout) {
                failure = Some(e);
                break;
            }
        }
        if let Some(e) = failure {
            self.restore(before);
            return Err(e);
        }
        if deleted_callout {
            self.renumber_compactly();
        }
        if self.annotations == before.annotations
            && self.next_number == before.next_number
            && self.next_id == before.next_id
        {
            return Ok(BatchOutcome { added_ids });
        }
        self.commit(before);
        Ok(BatchOutcome { added_ids })
    }

    fn apply_one(
        &mut self,
        op: EditOp,
        w: u32,
        h: u32,
        added_ids: &mut Vec<AnnotationId>,
        deleted_callout: &mut bool,
    ) -> Result<(), EditError> {
        match op {
            EditOp::AddTwoPoint {
                kind,
                start,
                end,
                style,
            } => {
                validate_stroke_style(style)?;
                let (start, end) = clamp_two_point(start, end, w, h)?;
                let id = self.allocate_id();
                self.annotations.push(Annotation::two_point_with_style(
                    id, kind, start, end, style,
                ));
                added_ids.push(id);
            }
            EditOp::AddRedaction { bounds } => {
                ensure_rect_finite(&bounds)?;
                let clamped = bounds.clamp_to(w, h);
                if clamped.is_empty() {
                    return Err(EditError::ZeroArea);
                }
                let id = self.allocate_id();
                self.annotations.push(Annotation::OpaqueRedaction {
                    id,
                    bounds: clamped,
                });
                added_ids.push(id);
            }
            EditOp::AddTextNote {
                position,
                text,
                style,
            } => {
                ensure_point_finite(&position)?;
                if text.trim().is_empty() {
                    return Err(EditError::EmptyText);
                }
                let id = self.allocate_id();
                self.annotations.push(Annotation::TextNote {
                    id,
                    position: position.clamp_to(w, h),
                    text,
                    style,
                });
                added_ids.push(id);
            }
            EditOp::AddNumberCallout { tip, bubble, style } => {
                ensure_point_finite(&tip)?;
                ensure_point_finite(&bubble)?;
                let id = self.allocate_id();
                let number = self.next_number;
                self.next_number += 1;
                self.annotations.push(Annotation::NumberCallout {
                    id,
                    number,
                    tip: tip.clamp_to(w, h),
                    bubble: bubble.clamp_to(w, h),
                    style,
                });
                added_ids.push(id);
            }
            EditOp::UpdateRedactionBounds { id, bounds } => {
                ensure_rect_finite(&bounds)?;
                let clamped = bounds.clamp_to(w, h);
                if clamped.is_empty() {
                    return Err(EditError::ZeroArea);
                }
                let index = self.annotation_index(id)?;
                match &mut self.annotations[index] {
                    Annotation::OpaqueRedaction { bounds: b, .. } => *b = clamped,
                    _ => return Err(EditError::WrongKind),
                }
            }
            EditOp::UpdateTextPosition { id, position } => {
                ensure_point_finite(&position)?;
                let index = self.annotation_index(id)?;
                match &mut self.annotations[index] {
                    Annotation::TextNote { position: p, .. } => *p = position.clamp_to(w, h),
                    _ => return Err(EditError::WrongKind),
                }
            }
            EditOp::UpdateText { id, text } => {
                if text.trim().is_empty() {
                    return Err(EditError::EmptyText);
                }
                let index = self.annotation_index(id)?;
                match &mut self.annotations[index] {
                    Annotation::TextNote { text: t, .. } => *t = text,
                    _ => return Err(EditError::WrongKind),
                }
            }
            EditOp::UpdateNumberPoints { id, tip, bubble } => {
                ensure_point_finite(&tip)?;
                ensure_point_finite(&bubble)?;
                let index = self.annotation_index(id)?;
                match &mut self.annotations[index] {
                    Annotation::NumberCallout {
                        tip: t, bubble: b, ..
                    } => {
                        *t = tip.clamp_to(w, h);
                        *b = bubble.clamp_to(w, h);
                    }
                    _ => return Err(EditError::WrongKind),
                }
            }
            EditOp::UpdateTwoPointPoints { id, start, end } => {
                let (start, end) = clamp_two_point(start, end, w, h)?;
                let index = self.annotation_index(id)?;
                match &mut self.annotations[index] {
                    Annotation::TwoPoint {
                        start: current_start,
                        end: current_end,
                        ..
                    } => {
                        *current_start = start;
                        *current_end = end;
                    }
                    _ => return Err(EditError::WrongKind),
                }
            }
            EditOp::UpdateStrokeStyle { id, style } => {
                validate_stroke_style(style)?;
                let index = self.annotation_index(id)?;
                match &mut self.annotations[index] {
                    Annotation::TwoPoint {
                        style: current_style,
                        ..
                    }
                    | Annotation::Freehand {
                        style: current_style,
                        ..
                    } => *current_style = style,
                    _ => return Err(EditError::WrongKind),
                }
            }
            EditOp::Delete { id } => {
                let index = self.annotation_index(id)?;
                let removed = self.annotations.remove(index);
                if matches!(removed, Annotation::NumberCallout { .. }) {
                    *deleted_callout = true;
                }
            }
            EditOp::UpdateNumberStyle { id, style } => {
                let index = self.annotation_index(id)?;
                match &mut self.annotations[index] {
                    Annotation::NumberCallout { style: s, .. } => {
                        if *s != style {
                            *s = style;
                        }
                    }
                    _ => return Err(EditError::WrongKind),
                }
            }
            EditOp::UpdateTextStyle { id, style } => {
                let index = self.annotation_index(id)?;
                match &mut self.annotations[index] {
                    Annotation::TextNote { style: s, .. } => {
                        if *s != style {
                            *s = style;
                        }
                    }
                    _ => return Err(EditError::WrongKind),
                }
            }
            EditOp::SetNextNumber { value } => {
                if value == 0 {
                    return Err(EditError::InvalidNextNumber);
                }
                if self.next_number == value {
                    return Ok(());
                }
                self.next_number = value;
            }
            EditOp::AddShape {
                kind,
                bounds,
                stroke,
                fill,
            } => {
                validate_stroke_style(stroke)?;
                ensure_rect_finite(&bounds)?;
                let clamped = bounds.clamp_to(w, h);
                if clamped.width <= 0.0 || clamped.height <= 0.0 {
                    return Err(EditError::InvalidShapeBounds);
                }
                let id = self.allocate_id();
                self.annotations.push(Annotation::Shape {
                    id,
                    kind,
                    bounds: clamped,
                    stroke,
                    fill,
                });
                added_ids.push(id);
            }
            EditOp::UpdateShapeBounds { id, bounds } => {
                ensure_rect_finite(&bounds)?;
                let clamped = bounds.clamp_to(w, h);
                if clamped.width <= 0.0 || clamped.height <= 0.0 {
                    return Err(EditError::InvalidShapeBounds);
                }
                let index = self.annotation_index(id)?;
                match &mut self.annotations[index] {
                    Annotation::Shape { bounds: b, .. } => *b = clamped,
                    _ => return Err(EditError::WrongKind),
                }
            }
            EditOp::UpdateShapeStyle { id, stroke, fill } => {
                validate_stroke_style(stroke)?;
                let index = self.annotation_index(id)?;
                match &mut self.annotations[index] {
                    Annotation::Shape {
                        stroke: s, fill: f, ..
                    } => {
                        *s = stroke;
                        *f = fill;
                    }
                    _ => return Err(EditError::WrongKind),
                }
            }
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
            EditOp::AddPixelate { bounds, block_size } => {
                use crate::pixelate::{MAX_PIXELATE_BLOCK_SIZE, MIN_PIXELATE_BLOCK_SIZE};
                if !(MIN_PIXELATE_BLOCK_SIZE..=MAX_PIXELATE_BLOCK_SIZE).contains(&block_size) {
                    return Err(EditError::InvalidPixelateBlockSize(block_size));
                }
                ensure_rect_finite(&bounds)?;
                let clamped = bounds.clamp_to(w, h);
                if clamped.is_empty() {
                    return Err(EditError::InvalidPixelateBounds);
                }
                let id = self.allocate_id();
                self.annotations.push(Annotation::Pixelate {
                    id,
                    bounds: clamped,
                    block_size,
                });
                added_ids.push(id);
            }
            EditOp::UpdatePixelateBounds { id, bounds } => {
                ensure_rect_finite(&bounds)?;
                let clamped = bounds.clamp_to(w, h);
                if clamped.is_empty() {
                    return Err(EditError::InvalidPixelateBounds);
                }
                let index = self.annotation_index(id)?;
                match &mut self.annotations[index] {
                    Annotation::Pixelate { bounds: b, .. } => {
                        if *b == clamped {
                            return Ok(());
                        }
                        *b = clamped;
                    }
                    _ => return Err(EditError::WrongKind),
                }
            }
            EditOp::UpdatePixelateBlockSize { id, block_size } => {
                use crate::pixelate::{MAX_PIXELATE_BLOCK_SIZE, MIN_PIXELATE_BLOCK_SIZE};
                if !(MIN_PIXELATE_BLOCK_SIZE..=MAX_PIXELATE_BLOCK_SIZE).contains(&block_size) {
                    return Err(EditError::InvalidPixelateBlockSize(block_size));
                }
                let index = self.annotation_index(id)?;
                match &mut self.annotations[index] {
                    Annotation::Pixelate { block_size: bs, .. } => {
                        if *bs == block_size {
                            return Ok(());
                        }
                        *bs = block_size;
                    }
                    _ => return Err(EditError::WrongKind),
                }
            }
        }
        Ok(())
    }

    /// Reassign callout numbers to 1..=n preserving current relative order;
    /// next allocation becomes n + 1.
    fn renumber_compactly(&mut self) {
        let mut callout_indices: Vec<usize> = self
            .annotations
            .iter()
            .enumerate()
            .filter(|(_, a)| matches!(a, Annotation::NumberCallout { .. }))
            .map(|(i, _)| i)
            .collect();
        callout_indices.sort_by_key(|&i| match &self.annotations[i] {
            Annotation::NumberCallout { number, .. } => *number,
            _ => unreachable!(),
        });
        for (new_number, &i) in callout_indices.iter().enumerate() {
            if let Annotation::NumberCallout { number, .. } = &mut self.annotations[i] {
                *number = new_number as u32 + 1;
            }
        }
        self.next_number = callout_indices.len() as u32 + 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::annotation::{ShapeKind, TwoPointKind};
    use crate::geometry::{ImagePoint, ImageRect, Rgb8};
    use crate::style::{NumberSize, NumberStyle, StrokeStyle, TextStyle};
    use image::{Rgba, RgbaImage};

    pub(crate) fn doc() -> ImageDocument {
        ImageDocument::new(RgbaImage::from_pixel(100, 200, Rgba([10, 20, 30, 255])))
    }

    fn image() -> RgbaImage {
        RgbaImage::from_pixel(100, 200, Rgba([10, 20, 30, 255]))
    }

    #[test]
    fn two_point_add_update_style_delete_undo_redo_is_one_entry_per_edit() {
        let mut doc = ImageDocument::new(image());
        let id = doc
            .add_two_point(
                TwoPointKind::Arrow,
                ImagePoint::new(10.0, 10.0),
                ImagePoint::new(80.0, 40.0),
            )
            .unwrap();
        doc.set_two_point_points(id, ImagePoint::new(20.0, 20.0), ImagePoint::new(90.0, 60.0))
            .unwrap();
        let style = StrokeStyle {
            color: Rgb8::new(1, 2, 3),
            width: 8.0,
            opacity: 1.0,
        };
        doc.set_stroke_style(id, style).unwrap();
        doc.delete_annotation(id).unwrap();
        assert!(doc.undo());
        assert_eq!(
            doc.annotation(id).and_then(Annotation::stroke_style),
            Some(style)
        );
        assert!(doc.redo());
        assert!(doc.annotation(id).is_none());
    }

    #[test]
    fn rejected_two_point_edits_are_atomic() {
        let mut doc = ImageDocument::new(image());
        let before_state = doc.state_id();
        assert_eq!(
            doc.add_two_point(
                TwoPointKind::Line,
                ImagePoint::new(5.0, 5.0),
                ImagePoint::new(5.0, 5.0),
            ),
            Err(EditError::CoincidentPoints)
        );
        assert_eq!(doc.state_id(), before_state);
        assert!(doc.annotations().is_empty());
    }

    #[test]
    fn invalid_stroke_values_are_rejected_without_mutation() {
        let mut doc = ImageDocument::new(image());
        for style in [
            StrokeStyle {
                width: 0.0,
                ..StrokeStyle::default()
            },
            StrokeStyle {
                width: f32::NAN,
                ..StrokeStyle::default()
            },
            StrokeStyle {
                opacity: -0.1,
                ..StrokeStyle::default()
            },
            StrokeStyle {
                opacity: 1.1,
                ..StrokeStyle::default()
            },
        ] {
            assert!(doc
                .add_two_point_with_style(
                    TwoPointKind::Line,
                    ImagePoint::new(1.0, 1.0),
                    ImagePoint::new(20.0, 20.0),
                    style,
                )
                .is_err());
        }
        assert!(doc.annotations().is_empty());
    }

    #[test]
    fn failed_batch_restores_next_id_and_noop_updates_create_no_history() {
        let mut doc = ImageDocument::new(image());
        let result = doc.apply_batch(vec![
            EditOp::AddTwoPoint {
                kind: TwoPointKind::Line,
                start: ImagePoint::new(1.0, 1.0),
                end: ImagePoint::new(20.0, 20.0),
                style: StrokeStyle::default(),
            },
            EditOp::AddTwoPoint {
                kind: TwoPointKind::Arrow,
                start: ImagePoint::new(5.0, 5.0),
                end: ImagePoint::new(5.0, 5.0),
                style: StrokeStyle::default(),
            },
        ]);
        assert_eq!(result, Err(EditError::CoincidentPoints));
        let id = doc
            .add_two_point(
                TwoPointKind::Line,
                ImagePoint::new(1.0, 1.0),
                ImagePoint::new(20.0, 20.0),
            )
            .unwrap();
        assert_eq!(id, AnnotationId(1));
        while doc.undo() {}
        let id = doc
            .add_two_point(
                TwoPointKind::Line,
                ImagePoint::new(1.0, 1.0),
                ImagePoint::new(20.0, 20.0),
            )
            .unwrap();
        let before = doc.state_id();
        doc.set_stroke_style(id, StrokeStyle::default()).unwrap();
        assert_eq!(doc.state_id(), before);
    }

    #[test]
    fn identical_two_point_and_raw_style_updates_create_no_history() {
        let mut doc = ImageDocument::new(image());
        let start = ImagePoint::new(1.0, 1.0);
        let end = ImagePoint::new(20.0, 20.0);
        let id = doc.add_two_point(TwoPointKind::Line, start, end).unwrap();
        let before = doc.state_id();

        doc.set_two_point_points(id, start, end).unwrap();
        doc.apply_batch(vec![EditOp::UpdateStrokeStyle {
            id,
            style: StrokeStyle::default(),
        }])
        .unwrap();

        assert_eq!(doc.state_id(), before);
        assert!(doc.undo());
        assert!(doc.annotation(id).is_none());
        assert!(!doc.can_undo());
    }

    #[test]
    fn new_document_is_empty_with_number_sequence_at_one() {
        let d = doc();
        assert!(d.annotations().is_empty());
        assert_eq!(d.next_number(), 1);
        assert!(!d.can_undo() && !d.can_redo());
    }

    #[test]
    fn add_number_callouts_allocates_sequential_numbers_and_unique_ids() {
        let mut d = doc();
        let a = d.add_number_callout(ImagePoint::new(1.0, 1.0), ImagePoint::new(1.0, 1.0));
        let b = d.add_number_callout(ImagePoint::new(2.0, 2.0), ImagePoint::new(9.0, 9.0));
        assert_ne!(a, b);
        let numbers: Vec<u32> = d
            .annotations()
            .iter()
            .map(|ann| match ann {
                Annotation::NumberCallout { number, .. } => *number,
                _ => panic!("expected number callout"),
            })
            .collect();
        assert_eq!(numbers, vec![1, 2]);
        assert_eq!(d.next_number(), 3);
    }

    #[test]
    fn add_text_note_rejects_whitespace_only_text() {
        let mut d = doc();
        assert_eq!(
            d.add_text_note(ImagePoint::new(5.0, 5.0), "   \n ".to_string()),
            Err(EditError::EmptyText)
        );
        assert!(d.annotations().is_empty());
        assert!(!d.can_undo(), "rejected edit must not enter history");
    }

    #[test]
    fn add_redaction_rejects_zero_area_after_clamp() {
        let mut d = doc();
        let zero = ImageRect {
            x: 5.0,
            y: 5.0,
            width: 0.4,
            height: 50.0,
        };
        assert_eq!(d.add_redaction(zero), Err(EditError::ZeroArea));
        // Entirely outside the image clamps to nothing.
        let outside = ImageRect {
            x: 500.0,
            y: 500.0,
            width: 50.0,
            height: 50.0,
        };
        assert_eq!(d.add_redaction(outside), Err(EditError::ZeroArea));
        assert!(d.annotations().is_empty());
    }

    #[test]
    fn add_clamps_geometry_into_image_bounds() {
        let mut d = doc();
        d.add_number_callout(ImagePoint::new(-10.0, 50.0), ImagePoint::new(150.0, 300.0));
        match &d.annotations()[0] {
            Annotation::NumberCallout { tip, bubble, .. } => {
                assert_eq!(*tip, ImagePoint::new(0.0, 50.0));
                assert_eq!(*bubble, ImagePoint::new(100.0, 200.0));
            }
            _ => panic!("expected number callout"),
        }
    }

    #[test]
    fn source_pixels_unchanged_by_edits() {
        let mut d = doc();
        let before = d.source().clone();
        d.add_number_callout(ImagePoint::new(1.0, 1.0), ImagePoint::new(2.0, 2.0));
        let _ = d.add_text_note(ImagePoint::new(5.0, 5.0), "note".to_string());
        let _ = d.add_redaction(ImageRect {
            x: 0.0,
            y: 0.0,
            width: 10.0,
            height: 10.0,
        });
        assert_eq!(d.source().as_raw(), before.as_raw());
    }

    #[test]
    fn state_id_changes_on_every_commit() {
        let mut d = doc();
        let s0 = d.state_id();
        d.add_number_callout(ImagePoint::new(1.0, 1.0), ImagePoint::new(1.0, 1.0));
        let s1 = d.state_id();
        assert_ne!(s0, s1);
        let _ = d.add_text_note(ImagePoint::new(5.0, 5.0), "x".to_string());
        assert_ne!(d.state_id(), s1);
    }

    #[test]
    fn undo_redo_restore_annotations_sequence_and_state_id() {
        let mut d = doc();
        let s0 = d.state_id();
        d.add_number_callout(ImagePoint::new(1.0, 1.0), ImagePoint::new(1.0, 1.0));
        let s1 = d.state_id();
        d.add_number_callout(ImagePoint::new(2.0, 2.0), ImagePoint::new(2.0, 2.0));

        assert!(d.undo());
        assert_eq!(d.annotations().len(), 1);
        assert_eq!(d.next_number(), 2, "sequence follows undo (spec §6)");
        assert_eq!(d.state_id(), s1);

        assert!(d.undo());
        assert!(d.annotations().is_empty());
        assert_eq!(d.next_number(), 1);
        assert_eq!(d.state_id(), s0);
        assert!(!d.undo(), "nothing left to undo");

        assert!(d.redo());
        assert_eq!(d.annotations().len(), 1);
        assert_eq!(d.next_number(), 2);
        assert_eq!(d.state_id(), s1);
    }

    #[test]
    fn new_edit_after_undo_clears_redo() {
        let mut d = doc();
        d.add_number_callout(ImagePoint::new(1.0, 1.0), ImagePoint::new(1.0, 1.0));
        d.add_number_callout(ImagePoint::new(2.0, 2.0), ImagePoint::new(2.0, 2.0));
        assert!(d.undo());
        assert!(d.can_redo());
        d.add_number_callout(ImagePoint::new(3.0, 3.0), ImagePoint::new(3.0, 3.0));
        assert!(!d.can_redo(), "spec §10: new edit clears redo");
    }

    #[test]
    fn history_caps_at_limit_dropping_oldest() {
        let mut d = doc();
        for i in 0..(HISTORY_LIMIT + 10) {
            d.add_number_callout(
                ImagePoint::new(i as f32 % 90.0, 1.0),
                ImagePoint::new(i as f32 % 90.0, 1.0),
            );
        }
        let mut undone = 0;
        while d.undo() {
            undone += 1;
        }
        assert_eq!(undone, HISTORY_LIMIT);
        assert_eq!(
            d.annotations().len(),
            10,
            "oldest 10 edits fell off the stack"
        );
    }

    #[test]
    fn ids_and_next_id_restore_across_undo_redo() {
        let mut d = doc();
        let first = d.add_number_callout(ImagePoint::new(1.0, 1.0), ImagePoint::new(1.0, 1.0));
        assert!(d.undo());
        let second = d.add_number_callout(ImagePoint::new(2.0, 2.0), ImagePoint::new(2.0, 2.0));
        assert_eq!(first, second, "undo restores the complete allocation state");
        assert!(d.undo());
        assert!(d.redo());
        assert_eq!(d.annotations()[0].id(), second, "redo restores the same id");
    }

    #[test]
    fn setters_update_geometry_and_are_undoable() {
        let mut d = doc();
        let id = d.add_number_callout(ImagePoint::new(5.0, 5.0), ImagePoint::new(5.0, 5.0));
        d.set_number_points(id, ImagePoint::new(10.0, 10.0), ImagePoint::new(40.0, 40.0))
            .unwrap();
        match d.annotation(id).unwrap() {
            Annotation::NumberCallout { tip, bubble, .. } => {
                assert_eq!(*tip, ImagePoint::new(10.0, 10.0));
                assert_eq!(*bubble, ImagePoint::new(40.0, 40.0));
            }
            _ => panic!(),
        }
        assert!(d.undo());
        match d.annotation(id).unwrap() {
            Annotation::NumberCallout { tip, .. } => assert_eq!(*tip, ImagePoint::new(5.0, 5.0)),
            _ => panic!(),
        }
    }

    #[test]
    fn unchanged_setter_is_a_no_op_without_history_entry() {
        let mut d = doc();
        let id = d.add_number_callout(ImagePoint::new(5.0, 5.0), ImagePoint::new(6.0, 6.0));
        let s = d.state_id();
        d.set_number_points(id, ImagePoint::new(5.0, 5.0), ImagePoint::new(6.0, 6.0))
            .unwrap();
        assert_eq!(d.state_id(), s, "no-op edit must not commit");
    }

    #[test]
    fn set_text_replaces_content_and_rejects_empty() {
        let mut d = doc();
        let id = d
            .add_text_note(ImagePoint::new(5.0, 5.0), "old".to_string())
            .unwrap();
        d.set_text(id, "new".to_string()).unwrap();
        match d.annotation(id).unwrap() {
            Annotation::TextNote { text, .. } => assert_eq!(text, "new"),
            _ => panic!(),
        }
        assert_eq!(d.set_text(id, "  ".to_string()), Err(EditError::EmptyText));
    }

    #[test]
    fn wrong_kind_and_unknown_id_are_rejected() {
        let mut d = doc();
        let id = d
            .add_text_note(ImagePoint::new(5.0, 5.0), "x".to_string())
            .unwrap();
        assert_eq!(
            d.set_number_points(id, ImagePoint::new(0.0, 0.0), ImagePoint::new(0.0, 0.0)),
            Err(EditError::WrongKind)
        );
        assert_eq!(
            d.delete_annotation(AnnotationId(999)),
            Err(EditError::UnknownAnnotation)
        );
    }

    #[test]
    fn set_redaction_bounds_resizes_and_rejects_zero_area() {
        let mut d = doc();
        let id = d
            .add_redaction(ImageRect {
                x: 1.0,
                y: 1.0,
                width: 10.0,
                height: 10.0,
            })
            .unwrap();
        d.set_redaction_bounds(
            id,
            ImageRect {
                x: 2.0,
                y: 2.0,
                width: 20.0,
                height: 5.0,
            },
        )
        .unwrap();
        assert_eq!(
            d.set_redaction_bounds(
                id,
                ImageRect {
                    x: 2.0,
                    y: 2.0,
                    width: 0.1,
                    height: 5.0
                }
            ),
            Err(EditError::ZeroArea)
        );
    }

    // -- D1: compact renumbering on delete ------------------------------------

    fn numbers(d: &ImageDocument) -> Vec<u32> {
        d.annotations()
            .iter()
            .filter_map(|a| match a {
                Annotation::NumberCallout { number, .. } => Some(*number),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn deleting_a_middle_callout_renumbers_compactly() {
        let mut d = doc();
        let _one = d.add_number_callout(ImagePoint::new(1.0, 1.0), ImagePoint::new(1.0, 1.0));
        let two = d.add_number_callout(ImagePoint::new(2.0, 2.0), ImagePoint::new(2.0, 2.0));
        let _three = d.add_number_callout(ImagePoint::new(3.0, 3.0), ImagePoint::new(3.0, 3.0));

        d.delete_annotation(two).unwrap();
        assert_eq!(numbers(&d), vec![1, 2], "1,2,3 minus #2 compacts to 1,2");
        assert_eq!(
            d.next_number(),
            3,
            "next allocation is highest remaining + 1"
        );
    }

    #[test]
    fn delete_then_create_allocates_highest_remaining_plus_one() {
        let mut d = doc();
        d.add_number_callout(ImagePoint::new(1.0, 1.0), ImagePoint::new(1.0, 1.0));
        let two = d.add_number_callout(ImagePoint::new(2.0, 2.0), ImagePoint::new(2.0, 2.0));
        d.add_number_callout(ImagePoint::new(3.0, 3.0), ImagePoint::new(3.0, 3.0));
        d.delete_annotation(two).unwrap();
        d.add_number_callout(ImagePoint::new(4.0, 4.0), ImagePoint::new(4.0, 4.0));
        assert_eq!(numbers(&d), vec![1, 2, 3]);
    }

    #[test]
    fn undo_of_delete_restores_exact_prior_numbering_in_one_step() {
        let mut d = doc();
        d.add_number_callout(ImagePoint::new(1.0, 1.0), ImagePoint::new(1.0, 1.0));
        let two = d.add_number_callout(ImagePoint::new(2.0, 2.0), ImagePoint::new(2.0, 2.0));
        d.add_number_callout(ImagePoint::new(3.0, 3.0), ImagePoint::new(3.0, 3.0));
        d.delete_annotation(two).unwrap();
        assert!(d.undo(), "delete + renumber is ONE history entry");
        assert_eq!(numbers(&d), vec![1, 2, 3]);
        assert_eq!(d.next_number(), 4);
        assert_eq!(
            d.annotations()[1].id(),
            two,
            "identity preserved through undo"
        );
    }

    #[test]
    fn edit_op_variants_construct_and_compare() {
        let a = EditOp::AddRedaction {
            bounds: ImageRect::from_corners(ImagePoint::new(0.0, 0.0), ImagePoint::new(10.0, 10.0)),
        };
        let b = a.clone();
        assert_eq!(a, b);
        let outcome = BatchOutcome {
            added_ids: vec![AnnotationId(1)],
        };
        assert_eq!(outcome.added_ids, vec![AnnotationId(1)]);
    }

    #[test]
    fn non_finite_coordinate_error_has_message() {
        assert_eq!(
            EditError::NonFiniteCoordinate.to_string(),
            "coordinates must be finite"
        );
    }

    #[test]
    fn is_finite_detects_nan_and_infinity() {
        assert!(ImagePoint::new(1.0, 2.0).is_finite());
        assert!(!ImagePoint::new(f32::NAN, 2.0).is_finite());
        let good = ImageRect::from_corners(ImagePoint::new(0.0, 0.0), ImagePoint::new(4.0, 4.0));
        assert!(good.is_finite());
        let bad = ImageRect {
            x: 0.0,
            y: 0.0,
            width: f32::INFINITY,
            height: 4.0,
        };
        assert!(!bad.is_finite());
    }

    #[test]
    fn deleting_last_callout_resets_sequence_and_non_number_delete_does_not_renumber() {
        let mut d = doc();
        let n = d.add_number_callout(ImagePoint::new(1.0, 1.0), ImagePoint::new(1.0, 1.0));
        let t = d
            .add_text_note(ImagePoint::new(5.0, 5.0), "x".to_string())
            .unwrap();
        d.delete_annotation(t).unwrap();
        assert_eq!(numbers(&d), vec![1], "text delete leaves numbering alone");
        d.delete_annotation(n).unwrap();
        assert_eq!(
            d.next_number(),
            1,
            "no callouts left → sequence restarts at 1"
        );
    }

    fn test_doc() -> ImageDocument {
        ImageDocument::new(image::RgbaImage::new(100, 100))
    }
    fn rect(x: f32, y: f32, w: f32, h: f32) -> ImageRect {
        ImageRect::from_corners(ImagePoint::new(x, y), ImagePoint::new(x + w, y + h))
    }

    #[test]
    fn apply_batch_of_adds_is_one_undo_entry() {
        let mut d = test_doc();
        let s_before = d.state_id();
        let out = d
            .apply_batch(vec![
                EditOp::AddRedaction {
                    bounds: rect(0.0, 0.0, 10.0, 10.0),
                },
                EditOp::AddRedaction {
                    bounds: rect(20.0, 20.0, 10.0, 10.0),
                },
                EditOp::AddRedaction {
                    bounds: rect(40.0, 40.0, 10.0, 10.0),
                },
            ])
            .expect("valid batch");
        assert_eq!(out.added_ids.len(), 3);
        assert_eq!(d.annotations().len(), 3);
        assert_eq!(
            d.state_id(),
            s_before + 1,
            "exactly one commit for the whole batch"
        );
        // ONE undo restores the EXACT pre-batch state (annotations + next_number + state_id).
        assert!(d.undo());
        assert_eq!(d.annotations().len(), 0);
        assert_eq!(d.next_number(), 1, "next_number restored");
        assert_eq!(d.state_id(), 0, "state_id restored to pre-batch");
        assert!(!d.can_undo());
    }

    #[test]
    fn apply_batch_is_atomic_on_invalid_op() {
        let mut d = test_doc();
        let state_before = d.state_id();
        let err = d
            .apply_batch(vec![
                EditOp::AddRedaction {
                    bounds: rect(0.0, 0.0, 10.0, 10.0),
                },
                EditOp::AddRedaction {
                    bounds: rect(0.0, 0.0, 0.0, 0.0),
                }, // zero area -> reject whole batch
            ])
            .unwrap_err();
        assert_eq!(err, EditError::ZeroArea);
        assert_eq!(d.annotations().len(), 0, "no partial mutation");
        assert_eq!(d.state_id(), state_before, "state_id unchanged");
        assert!(!d.can_undo());
    }

    #[test]
    fn apply_batch_rejects_non_finite() {
        let mut d = test_doc();
        let err = d
            .apply_batch(vec![EditOp::AddRedaction {
                bounds: ImageRect {
                    x: f32::NAN,
                    y: 0.0,
                    width: 5.0,
                    height: 5.0,
                },
            }])
            .unwrap_err();
        assert_eq!(err, EditError::NonFiniteCoordinate);
        assert_eq!(d.annotations().len(), 0);
    }

    #[test]
    fn apply_batch_empty_is_noop_without_history() {
        let mut d = test_doc();
        let out = d.apply_batch(vec![]).expect("empty ok");
        assert!(out.added_ids.is_empty());
        assert!(!d.can_undo());
        assert_eq!(d.state_id(), 0);
    }

    #[test]
    fn apply_batch_crud_and_callout_renumber_in_one_entry() {
        let mut d = test_doc();
        // Seed two callouts (numbers 1, 2) and one redaction via the batch path.
        let seed = d
            .apply_batch(vec![
                EditOp::AddNumberCallout {
                    tip: ImagePoint::new(1.0, 1.0),
                    bubble: ImagePoint::new(2.0, 2.0),
                    style: NumberStyle::default(),
                },
                EditOp::AddNumberCallout {
                    tip: ImagePoint::new(3.0, 3.0),
                    bubble: ImagePoint::new(4.0, 4.0),
                    style: NumberStyle::default(),
                },
                EditOp::AddRedaction {
                    bounds: rect(5.0, 5.0, 5.0, 5.0),
                },
            ])
            .expect("seed");
        let callout1 = seed.added_ids[0];
        let red = seed.added_ids[2];
        // Batch: delete callout #1 (forces renumber) + move the redaction. One entry.
        d.apply_batch(vec![
            EditOp::Delete { id: callout1 },
            EditOp::UpdateRedactionBounds {
                id: red,
                bounds: rect(50.0, 50.0, 8.0, 8.0),
            },
        ])
        .expect("crud batch");
        // Remaining callout renumbered to 1.
        let remaining_numbers: Vec<u32> = d
            .annotations()
            .iter()
            .filter_map(|a| match a {
                Annotation::NumberCallout { number, .. } => Some(*number),
                _ => None,
            })
            .collect();
        assert_eq!(
            remaining_numbers,
            vec![1],
            "exactly one callout, renumbered to 1"
        );
        // One undo reverts BOTH the delete and the update.
        assert!(d.undo());
        assert_eq!(d.annotations().len(), 3);
    }

    #[test]
    fn apply_batch_unknown_id_rejected() {
        let mut d = test_doc();
        let err = d
            .apply_batch(vec![EditOp::Delete {
                id: AnnotationId(999),
            }])
            .unwrap_err();
        assert_eq!(err, EditError::UnknownAnnotation);
    }

    #[test]
    fn apply_batch_rejects_reference_to_id_added_in_same_batch() {
        let mut d = test_doc();
        let state_before = d.state_id();
        let err = d
            .apply_batch(vec![
                EditOp::AddTextNote {
                    position: ImagePoint::new(1.0, 1.0),
                    text: "original".into(),
                    style: TextStyle::default(),
                },
                EditOp::UpdateText {
                    id: AnnotationId(1),
                    text: "updated".into(),
                },
            ])
            .unwrap_err();
        assert_eq!(err, EditError::UnknownAnnotation);
        assert!(d.annotations().is_empty(), "whole batch must be rejected");
        assert_eq!(d.state_id(), state_before);
        assert!(!d.can_undo());
    }

    #[test]
    fn apply_batch_wrong_kind_rejected() {
        let mut d = test_doc();
        let id = d.add_redaction(rect(0.0, 0.0, 5.0, 5.0)).unwrap();
        let err = d
            .apply_batch(vec![EditOp::UpdateText {
                id,
                text: "x".into(),
            }])
            .unwrap_err();
        assert_eq!(err, EditError::WrongKind);
        assert_eq!(d.annotations().len(), 1, "no mutation on reject");
    }

    #[test]
    fn apply_batch_added_ids_follow_op_order() {
        let mut d = test_doc();
        let out = d
            .apply_batch(vec![
                EditOp::AddRedaction {
                    bounds: rect(0.0, 0.0, 5.0, 5.0),
                },
                EditOp::AddTextNote {
                    position: ImagePoint::new(2.0, 2.0),
                    text: "a".into(),
                    style: TextStyle::default(),
                },
                EditOp::AddNumberCallout {
                    tip: ImagePoint::new(3.0, 3.0),
                    bubble: ImagePoint::new(4.0, 4.0),
                    style: NumberStyle::default(),
                },
            ])
            .expect("valid mixed adds");
        let live: Vec<_> = d.annotations().iter().map(|a| a.id()).collect();
        assert_eq!(
            out.added_ids, live,
            "added_ids match created annotations in op order"
        );
        assert!(out.added_ids[0] < out.added_ids[1] && out.added_ids[1] < out.added_ids[2]);
    }

    #[test]
    fn apply_batch_rejects_empty_text_atomically() {
        let mut d = test_doc();
        let err = d
            .apply_batch(vec![
                EditOp::AddRedaction {
                    bounds: rect(0.0, 0.0, 5.0, 5.0),
                },
                EditOp::AddTextNote {
                    position: ImagePoint::new(1.0, 1.0),
                    text: "   ".into(),
                    style: TextStyle::default(),
                },
            ])
            .unwrap_err();
        assert_eq!(err, EditError::EmptyText);
        assert_eq!(d.annotations().len(), 0, "whole batch rolled back");
        assert!(!d.can_undo());
    }

    #[test]
    fn apply_batch_update_text_empty_rejected() {
        let mut d = test_doc();
        let id = d
            .add_text_note(ImagePoint::new(2.0, 2.0), "orig".into())
            .unwrap();
        let err = d
            .apply_batch(vec![EditOp::UpdateText {
                id,
                text: "  ".into(),
            }])
            .unwrap_err();
        assert_eq!(err, EditError::EmptyText);
        match d.annotation(id).unwrap() {
            Annotation::TextNote { text, .. } => assert_eq!(text, "orig"),
            _ => panic!("wrong kind"),
        }
    }

    #[test]
    fn apply_batch_add_callout_rejects_non_finite() {
        let mut d = test_doc();
        let err = d
            .apply_batch(vec![EditOp::AddNumberCallout {
                tip: ImagePoint::new(f32::INFINITY, 1.0),
                bubble: ImagePoint::new(2.0, 2.0),
                style: NumberStyle::default(),
            }])
            .unwrap_err();
        assert_eq!(err, EditError::NonFiniteCoordinate);
        assert_eq!(d.annotations().len(), 0);
    }

    #[test]
    fn apply_batch_exercises_text_and_callout_update_paths() {
        let mut d = test_doc();
        let seed = d
            .apply_batch(vec![
                EditOp::AddTextNote {
                    position: ImagePoint::new(5.0, 5.0),
                    text: "old".into(),
                    style: TextStyle::default(),
                },
                EditOp::AddNumberCallout {
                    tip: ImagePoint::new(1.0, 1.0),
                    bubble: ImagePoint::new(2.0, 2.0),
                    style: NumberStyle::default(),
                },
            ])
            .expect("seed");
        let text_id = seed.added_ids[0];
        let callout_id = seed.added_ids[1];
        d.apply_batch(vec![
            EditOp::UpdateText {
                id: text_id,
                text: "new".into(),
            },
            EditOp::UpdateTextPosition {
                id: text_id,
                position: ImagePoint::new(9.0, 9.0),
            },
            EditOp::UpdateNumberPoints {
                id: callout_id,
                tip: ImagePoint::new(7.0, 7.0),
                bubble: ImagePoint::new(8.0, 8.0),
            },
        ])
        .expect("updates");
        match d.annotation(text_id).unwrap() {
            Annotation::TextNote { text, position, .. } => {
                assert_eq!(text, "new");
                assert_eq!(*position, ImagePoint::new(9.0, 9.0));
            }
            _ => panic!("wrong kind"),
        }
        match d.annotation(callout_id).unwrap() {
            Annotation::NumberCallout { tip, bubble, .. } => {
                assert_eq!(*tip, ImagePoint::new(7.0, 7.0));
                assert_eq!(*bubble, ImagePoint::new(8.0, 8.0));
            }
            _ => panic!("wrong kind"),
        }
    }

    // --- Phase B: style edits and next-number ---

    #[test]
    fn style_edit_retains_id_and_is_one_undo_entry() {
        let mut d = test_doc();
        let id = d.add_number_callout(point(5.0), point(5.0));
        let before = d.state_id();
        let style = NumberStyle {
            accent: Rgb8::new(1, 2, 3),
            size: NumberSize::Large,
        };
        d.set_number_style(id, style).unwrap();
        assert_eq!(d.annotation(id).unwrap().number_style(), Some(style));
        assert_ne!(d.state_id(), before);
        assert!(d.undo());
        assert_eq!(
            d.annotation(id).unwrap().number_style(),
            Some(NumberStyle::default())
        );
    }

    #[test]
    fn next_number_is_document_local_validated_and_restored_exactly() {
        let mut d = test_doc();
        assert_eq!(d.set_next_number(0), Err(EditError::InvalidNextNumber));
        assert_eq!(d.next_number(), 1);
        d.set_next_number(7).unwrap();
        let id = d.add_number_callout(point(1.0), point(1.0));
        assert!(matches!(
            d.annotation(id),
            Some(Annotation::NumberCallout { number: 7, .. })
        ));
        assert!(d.undo());
        assert_eq!(d.next_number(), 7);
        assert!(d.undo());
        assert_eq!(d.next_number(), 1);
    }

    #[test]
    fn wrong_kind_style_edit_is_atomic() {
        let mut d = test_doc();
        let id = d.add_redaction(rect(0.0, 0.0, 5.0, 5.0)).unwrap();
        let state = d.state_id();
        assert_eq!(
            d.set_text_style(id, TextStyle::default()),
            Err(EditError::WrongKind)
        );
        assert_eq!(d.state_id(), state);
        assert!(!d.can_redo());
    }

    #[test]
    fn set_next_number_zero_rejected_no_undo_entry() {
        let mut d = test_doc();
        let s = d.state_id();
        assert_eq!(d.set_next_number(0), Err(EditError::InvalidNextNumber));
        assert_eq!(d.state_id(), s);
        assert!(!d.can_undo());
    }

    #[test]
    fn set_next_number_unchanged_is_noop() {
        let mut d = test_doc();
        let s = d.state_id();
        d.set_next_number(1).unwrap();
        assert_eq!(d.state_id(), s, "unchanged next_number must not commit");
    }

    #[test]
    fn text_style_edit_retains_id_and_is_undoable() {
        let mut d = test_doc();
        let id = d.add_text_note(point(5.0), "note".to_string()).unwrap();
        let before = d.state_id();
        let style = TextStyle {
            font_size: crate::style::TextSize::Px32,
            text_color: Rgb8::new(10, 20, 30),
            background: None,
        };
        d.set_text_style(id, style).unwrap();
        assert_eq!(d.annotation(id).unwrap().text_style(), Some(style));
        assert_ne!(d.state_id(), before);
        assert!(d.undo());
        assert_eq!(
            d.annotation(id).unwrap().text_style(),
            Some(TextStyle::default())
        );
    }

    #[test]
    fn add_number_callout_with_style_stores_custom_style() {
        let mut d = test_doc();
        let style = NumberStyle {
            accent: Rgb8::new(100, 200, 50),
            size: NumberSize::Small,
        };
        let id = d.add_number_callout_with_style(point(1.0), point(2.0), style);
        assert_eq!(d.annotation(id).unwrap().number_style(), Some(style));
    }

    #[test]
    fn add_text_note_with_style_stores_custom_style() {
        let mut d = test_doc();
        let style = TextStyle {
            font_size: crate::style::TextSize::Px14,
            text_color: Rgb8::new(5, 5, 5),
            background: None,
        };
        let id = d
            .add_text_note_with_style(point(1.0), "hi".to_string(), style)
            .unwrap();
        assert_eq!(d.annotation(id).unwrap().text_style(), Some(style));
    }

    #[test]
    fn apply_batch_with_styles_and_sequence() {
        let mut d = test_doc();
        let ns = NumberStyle {
            accent: Rgb8::new(1, 1, 1),
            size: NumberSize::Large,
        };
        let ts = TextStyle {
            font_size: crate::style::TextSize::Px24,
            text_color: Rgb8::new(2, 2, 2),
            background: None,
        };
        let out = d
            .apply_batch(vec![
                EditOp::AddNumberCallout {
                    tip: point(1.0),
                    bubble: point(2.0),
                    style: ns,
                },
                EditOp::AddTextNote {
                    position: point(3.0),
                    text: "x".into(),
                    style: ts,
                },
                EditOp::SetNextNumber { value: 5 },
            ])
            .expect("batch");
        let callout_id = out.added_ids[0];
        let text_id = out.added_ids[1];
        assert_eq!(d.annotation(callout_id).unwrap().number_style(), Some(ns));
        assert_eq!(d.annotation(text_id).unwrap().text_style(), Some(ts));
        assert_eq!(d.next_number(), 5);
        assert!(d.undo());
        assert_eq!(d.next_number(), 1);
        assert!(d.annotation(callout_id).is_none());
    }

    fn point(x: f32) -> ImagePoint {
        ImagePoint::new(x, x)
    }

    // --- Shape tests ---

    #[test]
    fn shape_add_canonical_and_explicit_fields() {
        let mut d = test_doc();
        let id = d
            .add_shape(ShapeKind::Rectangle, rect(10.0, 20.0, 30.0, 40.0))
            .unwrap();
        match d.annotation(id).unwrap() {
            Annotation::Shape {
                kind,
                bounds,
                stroke,
                fill,
                ..
            } => {
                assert_eq!(*kind, ShapeKind::Rectangle);
                assert_eq!(*bounds, rect(10.0, 20.0, 30.0, 40.0));
                assert_eq!(*stroke, StrokeStyle::default());
                assert_eq!(*fill, None);
            }
            _ => panic!("expected Shape"),
        }
    }

    #[test]
    fn shape_add_with_style_stores_explicit_fields() {
        let mut d = test_doc();
        let style = StrokeStyle {
            color: Rgb8::new(1, 2, 3),
            width: 7.0,
            opacity: 1.0,
        };
        let id = d
            .add_shape_with_style(
                ShapeKind::Ellipse,
                rect(10.0, 20.0, 30.0, 40.0),
                style,
                Some(Rgb8::new(4, 5, 6)),
            )
            .unwrap();
        match d.annotation(id).unwrap() {
            Annotation::Shape {
                kind,
                bounds,
                stroke,
                fill,
                ..
            } => {
                assert_eq!(*kind, ShapeKind::Ellipse);
                assert_eq!(*bounds, rect(10.0, 20.0, 30.0, 40.0));
                assert_eq!(*stroke, style);
                assert_eq!(*fill, Some(Rgb8::new(4, 5, 6)));
            }
            _ => panic!("expected Shape"),
        }
    }

    #[test]
    fn shape_ids_are_distinct_and_stable() {
        let mut d = test_doc();
        let a = d
            .add_shape(ShapeKind::Rectangle, rect(0.0, 0.0, 10.0, 10.0))
            .unwrap();
        let b = d
            .add_shape(ShapeKind::Ellipse, rect(20.0, 20.0, 10.0, 10.0))
            .unwrap();
        assert_ne!(a, b);
        assert_eq!(d.annotations()[0].id(), a);
        assert_eq!(d.annotations()[1].id(), b);
    }

    #[test]
    fn shape_set_bounds_updates_and_is_undoable() {
        let mut d = test_doc();
        let id = d
            .add_shape(ShapeKind::Rectangle, rect(10.0, 20.0, 30.0, 40.0))
            .unwrap();
        d.set_shape_bounds(id, rect(20.0, 30.0, 40.0, 50.0))
            .unwrap();
        match d.annotation(id).unwrap() {
            Annotation::Shape { bounds, .. } => {
                assert_eq!(*bounds, rect(20.0, 30.0, 40.0, 50.0));
            }
            _ => panic!("expected Shape"),
        }
        assert!(d.undo());
        match d.annotation(id).unwrap() {
            Annotation::Shape { bounds, .. } => {
                assert_eq!(*bounds, rect(10.0, 20.0, 30.0, 40.0));
            }
            _ => panic!("expected Shape"),
        }
    }

    #[test]
    fn shape_set_style_updates_and_is_undoable() {
        let mut d = test_doc();
        let id = d
            .add_shape(ShapeKind::Rectangle, rect(10.0, 20.0, 30.0, 40.0))
            .unwrap();
        let new_style = StrokeStyle {
            color: Rgb8::new(10, 20, 30),
            width: 8.0,
            opacity: 0.5,
        };
        d.set_shape_style(id, new_style, Some(Rgb8::new(1, 2, 3)))
            .unwrap();
        match d.annotation(id).unwrap() {
            Annotation::Shape { stroke, fill, .. } => {
                assert_eq!(*stroke, new_style);
                assert_eq!(*fill, Some(Rgb8::new(1, 2, 3)));
            }
            _ => panic!("expected Shape"),
        }
        assert!(d.undo());
        match d.annotation(id).unwrap() {
            Annotation::Shape { stroke, fill, .. } => {
                assert_eq!(*stroke, StrokeStyle::default());
                assert_eq!(*fill, None);
            }
            _ => panic!("expected Shape"),
        }
    }

    #[test]
    fn shape_rejects_non_finite_bounds() {
        let mut d = test_doc();
        let s = d.state_id();
        assert_eq!(
            d.add_shape(
                ShapeKind::Rectangle,
                ImageRect {
                    x: f32::NAN,
                    y: 0.0,
                    width: 10.0,
                    height: 10.0,
                }
            ),
            Err(EditError::NonFiniteCoordinate)
        );
        assert_eq!(d.state_id(), s);
        assert!(d.annotations().is_empty());
    }

    #[test]
    fn shape_rejects_zero_dimensions() {
        let mut d = test_doc();
        assert_eq!(
            d.add_shape(ShapeKind::Rectangle, rect(0.0, 0.0, 0.0, 10.0)),
            Err(EditError::InvalidShapeBounds)
        );
        assert_eq!(
            d.add_shape(ShapeKind::Rectangle, rect(0.0, 0.0, 10.0, 0.0)),
            Err(EditError::InvalidShapeBounds)
        );
    }

    #[test]
    fn shape_rejects_invalid_stroke_width() {
        let mut d = test_doc();
        let bad = StrokeStyle {
            width: 0.0,
            ..StrokeStyle::default()
        };
        assert_eq!(
            d.add_shape_with_style(ShapeKind::Rectangle, rect(0.0, 0.0, 10.0, 10.0), bad, None,),
            Err(EditError::InvalidStrokeWidth)
        );
    }

    #[test]
    fn shape_rejects_invalid_opacity() {
        let mut d = test_doc();
        let bad = StrokeStyle {
            opacity: 1.5,
            ..StrokeStyle::default()
        };
        assert_eq!(
            d.add_shape_with_style(ShapeKind::Rectangle, rect(0.0, 0.0, 10.0, 10.0), bad, None,),
            Err(EditError::InvalidOpacity)
        );
    }

    #[test]
    fn shape_wrong_id_rejected() {
        let mut d = test_doc();
        assert_eq!(
            d.set_shape_bounds(AnnotationId(999), rect(0.0, 0.0, 10.0, 10.0)),
            Err(EditError::UnknownAnnotation)
        );
    }

    #[test]
    fn shape_wrong_kind_rejected() {
        let mut d = test_doc();
        let id = d.add_redaction(rect(0.0, 0.0, 10.0, 10.0)).unwrap();
        assert_eq!(
            d.set_shape_bounds(id, rect(0.0, 0.0, 10.0, 10.0)),
            Err(EditError::WrongKind)
        );
    }

    #[test]
    fn shape_unchanged_bounds_is_noop() {
        let mut d = test_doc();
        let id = d
            .add_shape(ShapeKind::Rectangle, rect(10.0, 20.0, 30.0, 40.0))
            .unwrap();
        let s = d.state_id();
        d.set_shape_bounds(id, rect(10.0, 20.0, 30.0, 40.0))
            .unwrap();
        assert_eq!(d.state_id(), s, "unchanged bounds must not commit");
    }

    #[test]
    fn shape_unchanged_style_is_noop() {
        let mut d = test_doc();
        let id = d
            .add_shape(ShapeKind::Rectangle, rect(10.0, 20.0, 30.0, 40.0))
            .unwrap();
        let s = d.state_id();
        d.set_shape_style(id, StrokeStyle::default(), None).unwrap();
        assert_eq!(d.state_id(), s, "unchanged style must not commit");
    }

    #[test]
    fn shape_delete_undo_redo_lifecycle() {
        let mut d = test_doc();
        let id = d
            .add_shape(ShapeKind::Rectangle, rect(10.0, 20.0, 30.0, 40.0))
            .unwrap();
        d.delete_annotation(id).unwrap();
        assert!(d.annotation(id).is_none());
        assert!(d.undo());
        assert!(d.annotation(id).is_some());
        assert!(d.redo());
        assert!(d.annotation(id).is_none());
    }

    #[test]
    fn shape_redo_clears_on_new_edit() {
        let mut d = test_doc();
        let _ = d
            .add_shape(ShapeKind::Rectangle, rect(0.0, 0.0, 10.0, 10.0))
            .unwrap();
        assert!(d.undo());
        assert!(d.can_redo());
        let _ = d
            .add_shape(ShapeKind::Ellipse, rect(20.0, 20.0, 10.0, 10.0))
            .unwrap();
        assert!(!d.can_redo());
    }

    // --- Freehand tests ---

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
        assert_eq!(
            doc.add_freehand_with_style(
                crate::FreehandKind::Pen,
                vec![ImagePoint::new(1.0, 1.0)],
                style
            ),
            Err(EditError::InvalidFreehandPath)
        );
        assert_eq!(
            doc.add_freehand_with_style(
                crate::FreehandKind::Pen,
                vec![ImagePoint::new(1.0, 1.0), ImagePoint::new(1.0, 1.0)],
                style
            ),
            Err(EditError::InvalidFreehandPath)
        );
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

    #[test]
    fn freehand_delete_undo_redo_lifecycle() {
        let mut d = test_doc();
        let id = d
            .add_freehand_with_style(
                crate::FreehandKind::Pen,
                vec![ImagePoint::new(0.0, 0.0), ImagePoint::new(10.0, 0.0)],
                StrokeStyle::default(),
            )
            .unwrap();
        d.delete_annotation(id).unwrap();
        assert!(d.annotation(id).is_none());
        assert!(d.undo());
        assert!(d.annotation(id).is_some());
        assert!(d.redo());
        assert!(d.annotation(id).is_none());
    }

    #[test]
    fn freehand_redo_clears_on_new_edit() {
        let mut d = test_doc();
        let _ = d
            .add_freehand_with_style(
                crate::FreehandKind::Pen,
                vec![ImagePoint::new(0.0, 0.0), ImagePoint::new(10.0, 0.0)],
                StrokeStyle::default(),
            )
            .unwrap();
        assert!(d.undo());
        assert!(d.can_redo());
        let _ = d
            .add_freehand_with_style(
                crate::FreehandKind::Highlighter,
                vec![ImagePoint::new(5.0, 5.0), ImagePoint::new(15.0, 5.0)],
                StrokeStyle::highlighter_default(),
            )
            .unwrap();
        assert!(!d.can_redo());
    }

    #[test]
    fn freehand_rejected_update_rollback() {
        let mut d = test_doc();
        let id = d
            .add_freehand_with_style(
                crate::FreehandKind::Pen,
                vec![ImagePoint::new(0.0, 0.0), ImagePoint::new(10.0, 0.0)],
                StrokeStyle::default(),
            )
            .unwrap();
        let s = d.state_id();
        assert_eq!(
            d.set_freehand_points(id, vec![ImagePoint::new(1.0, 1.0)]),
            Err(EditError::InvalidFreehandPath)
        );
        assert_eq!(d.state_id(), s);
    }

    #[test]
    fn freehand_unchanged_points_no_history() {
        let mut d = test_doc();
        let pts = vec![ImagePoint::new(0.0, 0.0), ImagePoint::new(10.0, 0.0)];
        let id = d
            .add_freehand_with_style(
                crate::FreehandKind::Pen,
                pts.clone(),
                StrokeStyle::default(),
            )
            .unwrap();
        let s = d.state_id();
        d.set_freehand_points(id, pts).unwrap();
        assert_eq!(d.state_id(), s);
    }

    #[test]
    fn freehand_unchanged_style_no_history() {
        let mut d = test_doc();
        let id = d
            .add_freehand_with_style(
                crate::FreehandKind::Pen,
                vec![ImagePoint::new(0.0, 0.0), ImagePoint::new(10.0, 0.0)],
                StrokeStyle::default(),
            )
            .unwrap();
        let s = d.state_id();
        d.set_stroke_style(id, StrokeStyle::default()).unwrap();
        assert_eq!(d.state_id(), s);
    }

    #[test]
    fn shared_source_arc_sharing_contract() {
        let d = doc();
        let a = d.shared_source();
        let b = d.shared_source();
        assert!(
            Arc::ptr_eq(&a, &b),
            "shared_source must return clones of the same Arc"
        );

        let before = d.source().as_raw().to_vec();
        let mut flat = d.flatten();
        // Mutate the flattened copy.
        for px in flat.pixels_mut() {
            px[0] = 255;
        }
        assert_eq!(
            d.source().as_raw(),
            before.as_slice(),
            "mutating a flattened copy must not change the document source"
        );
    }

    // --- Pixelate tests ---

    fn document_32_by_32() -> ImageDocument {
        ImageDocument::new(RgbaImage::from_pixel(32, 32, Rgba([10, 20, 30, 255])))
    }

    #[test]
    fn pixelate_default_construction_uses_default_block_size() {
        let mut d = document_32_by_32();
        let id = d
            .add_pixelate(ImageRect::new(2.0, 3.0, 12.0, 10.0), 16)
            .unwrap();
        match d.annotation(id) {
            Some(Annotation::Pixelate {
                bounds, block_size, ..
            }) => {
                assert_eq!(*bounds, ImageRect::new(2.0, 3.0, 12.0, 10.0));
                assert_eq!(*block_size, 16);
            }
            _ => panic!("expected Pixelate"),
        }
    }

    #[test]
    fn pixelate_stable_identity_across_updates() {
        let mut d = document_32_by_32();
        let id = d
            .add_pixelate(ImageRect::new(2.0, 3.0, 12.0, 10.0), 16)
            .unwrap();
        d.set_pixelate_bounds(id, ImageRect::new(4.0, 5.0, 8.0, 7.0))
            .unwrap();
        d.set_pixelate_block_size(id, 24).unwrap();
        assert_eq!(d.annotation(id).unwrap().id(), id);
    }

    #[test]
    fn pixelate_edits_are_validated_and_undo_as_single_entries() {
        let mut document = document_32_by_32();
        let id = document
            .add_pixelate(ImageRect::new(2.0, 3.0, 12.0, 10.0), 16)
            .unwrap();
        document
            .set_pixelate_bounds(id, ImageRect::new(4.0, 5.0, 8.0, 7.0))
            .unwrap();
        document.set_pixelate_block_size(id, 24).unwrap();
        assert!(matches!(
            document.annotation(id),
            Some(Annotation::Pixelate {
                bounds,
                block_size: 24,
                ..
            }) if *bounds == ImageRect::new(4.0, 5.0, 8.0, 7.0)
        ));
        assert!(document.undo());
        assert!(matches!(
            document.annotation(id),
            Some(Annotation::Pixelate { block_size: 16, .. })
        ));
        assert!(document.redo());
        assert!(matches!(
            document.annotation(id),
            Some(Annotation::Pixelate { block_size: 24, .. })
        ));
    }

    #[test]
    fn pixelate_delete_undo_redo_lifecycle() {
        let mut d = document_32_by_32();
        let id = d
            .add_pixelate(ImageRect::new(2.0, 3.0, 12.0, 10.0), 16)
            .unwrap();
        d.delete_annotation(id).unwrap();
        assert!(d.annotation(id).is_none());
        assert!(d.undo());
        assert!(d.annotation(id).is_some());
        assert!(d.redo());
        assert!(d.annotation(id).is_none());
    }

    #[test]
    fn pixelate_noop_bounds_update_creates_no_history() {
        let mut d = document_32_by_32();
        let bounds = ImageRect::new(2.0, 3.0, 12.0, 10.0);
        let id = d.add_pixelate(bounds, 16).unwrap();
        let s = d.state_id();
        d.set_pixelate_bounds(id, bounds).unwrap();
        assert_eq!(d.state_id(), s);
    }

    #[test]
    fn pixelate_noop_block_size_update_creates_no_history() {
        let mut d = document_32_by_32();
        let id = d
            .add_pixelate(ImageRect::new(2.0, 3.0, 12.0, 10.0), 16)
            .unwrap();
        let s = d.state_id();
        d.set_pixelate_block_size(id, 16).unwrap();
        assert_eq!(d.state_id(), s);
    }

    #[test]
    fn pixelate_rejects_block_size_3_and_49() {
        let mut d = document_32_by_32();
        assert_eq!(
            d.add_pixelate(ImageRect::new(1.0, 1.0, 5.0, 5.0), 3),
            Err(EditError::InvalidPixelateBlockSize(3))
        );
        assert_eq!(
            d.add_pixelate(ImageRect::new(1.0, 1.0, 5.0, 5.0), 49),
            Err(EditError::InvalidPixelateBlockSize(49))
        );
        assert!(d.annotations().is_empty());
        assert!(!d.can_undo());
    }

    #[test]
    fn pixelate_rejects_non_finite_bounds() {
        let mut d = document_32_by_32();
        assert_eq!(
            d.add_pixelate(
                ImageRect {
                    x: f32::NAN,
                    y: 0.0,
                    width: 5.0,
                    height: 5.0,
                },
                16
            ),
            Err(EditError::NonFiniteCoordinate)
        );
        assert!(d.annotations().is_empty());
    }

    #[test]
    fn pixelate_rejects_empty_bounds_after_clamp() {
        let mut d = document_32_by_32();
        // Entirely outside the image
        assert_eq!(
            d.add_pixelate(ImageRect::new(500.0, 500.0, 10.0, 10.0), 16),
            Err(EditError::InvalidPixelateBounds)
        );
        // Sub-pixel after clamp
        assert_eq!(
            d.add_pixelate(ImageRect::new(5.0, 5.0, 0.4, 50.0), 16),
            Err(EditError::InvalidPixelateBounds)
        );
        assert!(d.annotations().is_empty());
    }

    #[test]
    fn pixelate_wrong_kind_rejected() {
        let mut d = document_32_by_32();
        let id = d
            .add_redaction(ImageRect::new(1.0, 1.0, 10.0, 10.0))
            .unwrap();
        assert_eq!(
            d.set_pixelate_bounds(id, ImageRect::new(1.0, 1.0, 5.0, 5.0)),
            Err(EditError::WrongKind)
        );
        assert_eq!(d.set_pixelate_block_size(id, 16), Err(EditError::WrongKind));
    }

    #[test]
    fn invalid_pixelate_batch_is_atomic() {
        let mut document = document_32_by_32();
        let before = document.state_id();
        let result = document.apply_batch(vec![
            EditOp::AddPixelate {
                bounds: ImageRect::new(1.0, 1.0, 5.0, 5.0),
                block_size: 16,
            },
            EditOp::AddPixelate {
                bounds: ImageRect::new(2.0, 2.0, 5.0, 5.0),
                block_size: 49,
            },
        ]);
        assert_eq!(result, Err(EditError::InvalidPixelateBlockSize(49)));
        assert!(document.annotations().is_empty());
        assert_eq!(document.state_id(), before);
        assert!(!document.can_undo());
    }

    #[test]
    fn pixelate_redo_clears_on_new_edit() {
        let mut d = document_32_by_32();
        let _ = d
            .add_pixelate(ImageRect::new(1.0, 1.0, 10.0, 10.0), 16)
            .unwrap();
        assert!(d.undo());
        assert!(d.can_redo());
        let _ = d
            .add_pixelate(ImageRect::new(5.0, 5.0, 10.0, 10.0), 8)
            .unwrap();
        assert!(!d.can_redo());
    }
}
