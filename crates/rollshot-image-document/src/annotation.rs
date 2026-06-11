//! The annotation graph. Geometry is stored in full-resolution image
//! coordinates (spec §6); IDs are stable across undo/redo.

use crate::geometry::{ImagePoint, ImageRect};

/// Stable annotation identity, never reused within a document.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct AnnotationId(pub u64);

#[derive(Debug, Clone, PartialEq)]
pub enum Annotation {
    NumberCallout {
        id: AnnotationId,
        number: u32,
        /// The pointed-at location (leader tip).
        tip: ImagePoint,
        /// The number bubble center. Coincident with `tip` for a stamp.
        bubble: ImagePoint,
    },
    TextNote {
        id: AnnotationId,
        /// Top-left of the backing plate.
        position: ImagePoint,
        text: String,
    },
    OpaqueRedaction {
        id: AnnotationId,
        bounds: ImageRect,
    },
}

impl Annotation {
    pub fn id(&self) -> AnnotationId {
        match self {
            Annotation::NumberCallout { id, .. }
            | Annotation::TextNote { id, .. }
            | Annotation::OpaqueRedaction { id, .. } => *id,
        }
    }

    /// Reading-order anchor used for Navigator ordering (spec §8.2).
    pub fn anchor(&self) -> ImagePoint {
        match self {
            Annotation::NumberCallout { bubble, .. } => *bubble,
            Annotation::TextNote { position, .. } => *position,
            Annotation::OpaqueRedaction { bounds, .. } => ImagePoint::new(bounds.x, bounds.y),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::{ImagePoint, ImageRect};

    #[test]
    fn anchor_is_bubble_for_number_position_for_text_topleft_for_redaction() {
        let n = Annotation::NumberCallout {
            id: AnnotationId(1),
            number: 1,
            tip: ImagePoint::new(5.0, 5.0),
            bubble: ImagePoint::new(40.0, 60.0),
        };
        assert_eq!(n.anchor(), ImagePoint::new(40.0, 60.0));

        let t = Annotation::TextNote {
            id: AnnotationId(2),
            position: ImagePoint::new(7.0, 8.0),
            text: "hi".to_string(),
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
}
