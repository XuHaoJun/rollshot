//! The non-destructive image document: immutable source, annotation graph,
//! number sequence, and snapshot-based history (spec §6, §10).

use std::collections::VecDeque;

use image::RgbaImage;

use crate::annotation::{Annotation, AnnotationId};
use crate::geometry::{ImagePoint, ImageRect};

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
        self.annotations
            .push(Annotation::OpaqueRedaction { id, bounds: clamped });
        self.commit(before);
        Ok(id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
        assert_eq!(d.annotations().len(), 10, "oldest 10 edits fell off the stack");
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
}
