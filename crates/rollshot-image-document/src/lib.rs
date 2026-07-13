//! Headless, framework-neutral, non-destructive image document and editing
//! engine. Owns the immutable source image, the annotation graph, history,
//! geometry, and flattened rendering. Contains no UI, windowing, clipboard,
//! or capture code — see README.md for the responsibility boundary.

mod annotation;
pub mod callout_placement;
mod document;
mod edit_op;
mod flatten;
mod geometry;
mod hit;
mod navigator;
mod raster;
mod shapes;
pub mod style;
mod text;
mod two_point;

pub use annotation::{Annotation, AnnotationId, ShapeKind, TwoPointKind};
pub use callout_placement::place_number_callout_bubble;
pub use document::{EditError, ImageDocument, HISTORY_LIMIT};
pub use edit_op::{BatchOutcome, EditOp};
pub use geometry::{ImagePoint, ImageRect, Rgb8, Rgba8};
pub use hit::{hit_test_annotation, redaction_handles, Hit, HitPart, ResizeHandle};
pub use navigator::NavigatorItem;
pub use shapes::{annotation_bounds, annotation_shapes, text_plate_rect, RenderShape, TextAnchor};
pub use style::{NumberSize, NumberStyle, StrokeStyle, TextSize, TextStyle};
pub use text::{draw_text_block, measure_block};
pub use two_point::{arrowhead_points, point_in_triangle, segment_distance, two_point_bounds};
