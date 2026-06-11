//! Editor/session state for the Result Workspace (spec §5.2/§7): active tool,
//! selection, in-progress gesture drafts, and the inline text draft. None of
//! this enters the image document or its history.

use iced::widget::text_editor;
use iced::Point;
use rollshot_image_document::{
    Annotation, AnnotationId, HitPart, ImagePoint, ImageRect, ResizeHandle,
};
use std::time::Instant;

/// Screen-space hit tolerance; divide by the viewport scale for image space.
pub const HIT_TOLERANCE_SCREEN: f32 = 8.0;
/// Screen-space slop and window for double-click detection.
pub const DOUBLE_CLICK_SLOP_SCREEN: f32 = 6.0;
pub const DOUBLE_CLICK_WINDOW_MS: u128 = 400;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tool {
    Select,
    Number,
    Text,
    Redact,
}

/// An in-progress pointer gesture. Exactly ONE document edit is submitted on
/// release (spec §5.2); previews are rendered from this state only.
#[derive(Debug, Clone)]
pub enum DragState {
    /// Select-tool drag on empty canvas: pans via the scrollable.
    Pan { last_pointer: Point },
    /// Number tool: tip anchored at the press point, bubble follows the drag.
    CreateNumber { tip: ImagePoint, bubble: ImagePoint },
    CreateRedaction {
        anchor: ImagePoint,
        current: ImagePoint,
    },
    /// Select-tool drag of an existing annotation or one of its handles.
    EditAnnotation {
        part: HitPart,
        original: Annotation,
        /// press point − annotation reference point, so the body doesn't jump.
        grab_offset: (f32, f32),
        current: Annotation,
    },
}

/// The inline multi-line text editor draft (spec §9.3).
pub struct TextDraft {
    /// `Some(id)` when re-editing an existing note, `None` when creating.
    pub target: Option<AnnotationId>,
    pub position: ImagePoint,
    pub content: text_editor::Content,
}

pub struct EditorState {
    pub tool: Tool,
    pub selection: Option<AnnotationId>,
    pub drag: Option<DragState>,
    pub text_draft: Option<TextDraft>,
    pub navigator_open: bool,
    pub copy_menu_open: bool,
    /// Document `state_id` at the last successful Save As (dirty marker).
    pub saved_state_id: u64,
    /// Last canvas press, for double-click detection.
    pub last_press: Option<(Instant, ImagePoint)>,
}

impl EditorState {
    pub fn new(saved_state_id: u64, navigator_open: bool) -> Self {
        Self {
            tool: Tool::Select,
            selection: None,
            drag: None,
            text_draft: None,
            navigator_open,
            copy_menu_open: false,
            saved_state_id,
            last_press: None,
        }
    }
}

/// Pure drag-preview: the annotation as it would be committed if the pointer
/// released at `point`. Used by both the live draft rendering and the
/// release-commit, so preview and result cannot diverge.
pub fn dragged_annotation(
    original: &Annotation,
    part: HitPart,
    point: ImagePoint,
    grab_offset: (f32, f32),
) -> Annotation {
    let mut next = original.clone();
    match (&mut next, part) {
        (Annotation::NumberCallout { tip, .. }, HitPart::NumberTip) => *tip = point,
        (Annotation::NumberCallout { bubble, .. }, HitPart::NumberBubble) => *bubble = point,
        (Annotation::NumberCallout { tip, bubble, .. }, HitPart::Body) => {
            let dx = point.x - grab_offset.0 - bubble.x;
            let dy = point.y - grab_offset.1 - bubble.y;
            *tip = ImagePoint::new(tip.x + dx, tip.y + dy);
            *bubble = ImagePoint::new(bubble.x + dx, bubble.y + dy);
        }
        (Annotation::TextNote { position, .. }, HitPart::Body) => {
            *position = ImagePoint::new(point.x - grab_offset.0, point.y - grab_offset.1);
        }
        (Annotation::OpaqueRedaction { bounds, .. }, HitPart::Body) => {
            bounds.x = point.x - grab_offset.0;
            bounds.y = point.y - grab_offset.1;
        }
        (Annotation::OpaqueRedaction { bounds, .. }, HitPart::Resize(handle)) => {
            *bounds = resized_rect(*bounds, handle, point);
        }
        _ => {}
    }
    next
}

fn resized_rect(original: ImageRect, handle: ResizeHandle, p: ImagePoint) -> ImageRect {
    let left = original.x;
    let top = original.y;
    let right = original.x + original.width;
    let bottom = original.y + original.height;
    let (l, t, r, b) = match handle {
        ResizeHandle::TopLeft => (p.x, p.y, right, bottom),
        ResizeHandle::Top => (left, p.y, right, bottom),
        ResizeHandle::TopRight => (left, p.y, p.x, bottom),
        ResizeHandle::Right => (left, top, p.x, bottom),
        ResizeHandle::BottomRight => (left, top, p.x, p.y),
        ResizeHandle::Bottom => (left, top, right, p.y),
        ResizeHandle::BottomLeft => (p.x, top, right, p.y),
        ResizeHandle::Left => (p.x, top, right, bottom),
    };
    ImageRect::from_corners(ImagePoint::new(l, t), ImagePoint::new(r, b))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resize_from_each_side_normalizes_inverted_drags() {
        let rect = ImageRect {
            x: 10.0,
            y: 10.0,
            width: 20.0,
            height: 20.0,
        };
        let r = resized_rect(rect, ResizeHandle::Right, ImagePoint::new(50.0, 99.0));
        assert_eq!(
            r,
            ImageRect {
                x: 10.0,
                y: 10.0,
                width: 40.0,
                height: 20.0
            }
        );
        let flipped = resized_rect(rect, ResizeHandle::Right, ImagePoint::new(2.0, 99.0));
        assert_eq!(
            flipped,
            ImageRect {
                x: 2.0,
                y: 10.0,
                width: 8.0,
                height: 20.0
            }
        );
    }

    #[test]
    fn body_drag_preserves_grab_offset_and_moves_number_as_a_unit() {
        let original = Annotation::NumberCallout {
            id: AnnotationId(1),
            number: 1,
            tip: ImagePoint::new(0.0, 0.0),
            bubble: ImagePoint::new(10.0, 10.0),
        };
        let moved = dragged_annotation(
            &original,
            HitPart::Body,
            ImagePoint::new(25.0, 25.0),
            (5.0, 5.0),
        );
        match moved {
            Annotation::NumberCallout { tip, bubble, .. } => {
                assert_eq!(bubble, ImagePoint::new(20.0, 20.0));
                assert_eq!(tip, ImagePoint::new(10.0, 10.0));
            }
            _ => panic!(),
        }
    }
}
