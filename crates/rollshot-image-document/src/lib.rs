//! Headless, framework-neutral, non-destructive image document and editing
//! engine. Owns the immutable source image, the annotation graph, history,
//! geometry, and flattened rendering. Contains no UI, windowing, clipboard,
//! or capture code — see README.md for the responsibility boundary.

mod annotation;
mod document;
mod flatten;
mod geometry;
mod hit;
mod navigator;
mod raster;
mod shapes;
pub mod style;
mod text;

/* Uncommented as modules are implemented:
pub use annotation::{Annotation, AnnotationId};
pub use document::{EditError, ImageDocument, HISTORY_LIMIT};
pub use geometry::{ImagePoint, ImageRect, Rgba8};
pub use hit::{redaction_handles, Hit, HitPart, ResizeHandle};
pub use navigator::NavigatorItem;
pub use shapes::{
    annotation_bounds, annotation_shapes, text_plate_rect, RenderShape, TextAnchor,
};
pub use text::measure_block;
*/
