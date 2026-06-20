//! The non-destructive image document: immutable source, annotation graph,
//! number sequence, and snapshot-based history (spec §6, §10).

use std::collections::VecDeque;

use image::RgbaImage;

use crate::annotation::{Annotation, AnnotationId};
use crate::geometry::{ImagePoint, ImageRect};
use crate::hit::Hit;
use crate::navigator::NavigatorItem;

/// Maximum undo entries (spec §10).
pub const HISTORY_LIMIT: usize = 100;

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum EditError {
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
}

/// One restorable history state (mark-shot pattern: graph + counters).
#[derive(Debug, Clone)]
struct Snapshot {
    annotations: Vec<Annotation>,
    next_number: u32,
    state_id: u64,
}

pub struct ImageDocument {
    source: RgbaImage,
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
            source,
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

    pub fn add_number_callout(&mut self, tip: ImagePoint, bubble: ImagePoint) -> AnnotationId {
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
        });
        self.commit(before);
        id
    }

    pub fn add_text_note(
        &mut self,
        position: ImagePoint,
        text: String,
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
    use crate::edit_op::{BatchOutcome, EditOp};
    use crate::geometry::{ImagePoint, ImageRect};
    use image::{Rgba, RgbaImage};

    pub(crate) fn doc() -> ImageDocument {
        ImageDocument::new(RgbaImage::from_pixel(100, 200, Rgba([10, 20, 30, 255])))
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
    fn ids_stay_stable_across_undo_redo_and_are_never_reused() {
        let mut d = doc();
        let first = d.add_number_callout(ImagePoint::new(1.0, 1.0), ImagePoint::new(1.0, 1.0));
        assert!(d.undo());
        let second = d.add_number_callout(ImagePoint::new(2.0, 2.0), ImagePoint::new(2.0, 2.0));
        assert_ne!(first, second, "ids are never reused after undo");
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
        let outcome = BatchOutcome { added_ids: vec![AnnotationId(1)] };
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
        let bad = ImageRect { x: 0.0, y: 0.0, width: f32::INFINITY, height: 4.0 };
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
}
