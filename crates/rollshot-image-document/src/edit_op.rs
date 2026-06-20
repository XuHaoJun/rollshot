//! Typed, agent-free document edit operations and their batch outcome.
//! Applied atomically by `ImageDocument::apply_batch` (spec §6.5).

use crate::annotation::AnnotationId;
use crate::geometry::{ImagePoint, ImageRect};

/// A single document mutation. Add* allocate new ids; Update*/Delete reference
/// annotations that exist BEFORE the batch is applied.
#[derive(Debug, Clone, PartialEq)]
pub enum EditOp {
    AddRedaction { bounds: ImageRect },
    AddTextNote { position: ImagePoint, text: String },
    AddNumberCallout { tip: ImagePoint, bubble: ImagePoint },
    UpdateRedactionBounds { id: AnnotationId, bounds: ImageRect },
    UpdateTextPosition { id: AnnotationId, position: ImagePoint },
    UpdateText { id: AnnotationId, text: String },
    UpdateNumberPoints { id: AnnotationId, tip: ImagePoint, bubble: ImagePoint },
    Delete { id: AnnotationId },
}

/// Result of a successful `apply_batch`: ids allocated for the Add* ops, in the
/// order those ops appeared in the batch.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct BatchOutcome {
    pub added_ids: Vec<AnnotationId>,
}
