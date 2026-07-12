//! Editor/session state for the Result Workspace (spec §5.2/§7): active tool,
//! selection, in-progress gesture drafts, and the inline text draft. None of
//! this enters the image document or its history.
//!
//! Most types here are consumed by the pointer handlers and canvas rendering in
//! later tasks; dead-code warnings are suppressed until those are wired in.

#![allow(dead_code)]

use iced::widget::text_editor;
use iced::Point;
use rollshot_image_document::{
    Annotation, AnnotationId, HitPart, ImagePoint, ImageRect,
    ResizeHandle::{self, *},
};
use std::time::Instant;

use super::properties::PropertyState;

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
    #[cfg(feature = "ocr")]
    OcrText,
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
    /// Cached Navigator order, refreshed only when the document changes
    /// (spec §13). Keyed by the document state_id.
    pub navigator_items: Vec<rollshot_image_document::NavigatorItem>,
    pub navigator_items_state: Option<u64>,
    /// Transactional property editing state (color picker, next number input).
    pub properties: PropertyState,
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
            navigator_items: Vec::new(),
            navigator_items_state: None,
            properties: PropertyState::default(),
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

// ---------------------------------------------------------------------------
// Canvas program — annotation overlay with culling and event translation
// ---------------------------------------------------------------------------

use iced::widget::canvas;
use iced::{mouse, Color, Rectangle, Renderer, Size, Theme, Vector};
use rollshot_image_document::{
    annotation_bounds, annotation_shapes, redaction_handles, style, RenderShape, TextAnchor,
};

use super::update::{direct_manipulation_hit, Message};

/// Screen-space radius of selection handles (zoom-independent).
pub const HANDLE_RADIUS_SCREEN: f32 = 6.0;

pub(crate) fn token_color(c: rollshot_image_document::Rgba8) -> Color {
    Color::from_rgba8(c.r, c.g, c.b, c.a as f32 / 255.0)
}

const ANNOTATION_FONT: iced::Font = iced::Font::with_name(style::FONT_FAMILY_NAME);
const ANNOTATION_FONT_BOLD: iced::Font = iced::Font {
    weight: iced::font::Weight::Bold,
    ..iced::Font::with_name(style::FONT_FAMILY_NAME)
};

pub(crate) fn annotation_font() -> iced::Font {
    ANNOTATION_FONT
}

/// The portion of the image (image coordinates) currently visible in the
/// scrollable viewport — culling input (spec §11.1).
pub fn visible_image_rect(
    scroll_offset: Vector,
    viewport: Size,
    scale: f32,
    image_origin: Point,
) -> ImageRect {
    ImageRect {
        x: (scroll_offset.x - image_origin.x) / scale,
        y: (scroll_offset.y - image_origin.y) / scale,
        width: viewport.width / scale,
        height: viewport.height / scale,
    }
}

/// View-built canvas program: draws committed annotations (culled), the
/// active draft, and selection handles; translates pointer events into
/// image-space messages. All state lives in `ResultWorkspace`.
pub struct AnnotationCanvas<'a> {
    pub document: &'a rollshot_image_document::ImageDocument,
    pub editor: &'a EditorState,
    pub scale: f32,
    pub visible: ImageRect,
    // SP6 workbench candidate overlay. `None` in Normal mode.
    pub pending_proposal: Option<&'a rollshot_edit_proposal::EditProposal>,
    pub review: Option<&'a super::workbench::CandidateReview>,
    pub selected_candidate: Option<rollshot_edit_proposal::CandidateId>,
}

fn release_image_point(cursor: mouse::Cursor, bounds: Rectangle, scale: f32) -> Option<ImagePoint> {
    cursor
        .position_from(Point::new(bounds.x, bounds.y))
        .map(|local| ImagePoint::new(local.x / scale, local.y / scale))
}

impl AnnotationCanvas<'_> {
    fn image_point(&self, local: Point) -> ImagePoint {
        ImagePoint::new(local.x / self.scale, local.y / self.scale)
    }

    /// The annotation id whose committed visual is replaced by a draft.
    fn dragged_id(&self) -> Option<AnnotationId> {
        match &self.editor.drag {
            Some(DragState::EditAnnotation { original, .. }) => Some(original.id()),
            _ => None,
        }
    }

    fn draw_shape(&self, frame: &mut canvas::Frame, shape: &RenderShape) {
        let s = self.scale;
        match shape {
            RenderShape::Rect { rect, color } => frame.fill_rectangle(
                Point::new(rect.x * s, rect.y * s),
                Size::new(rect.width * s, rect.height * s),
                token_color(*color),
            ),
            RenderShape::Circle {
                center,
                radius,
                fill,
                outline_width,
                outline,
            } => {
                let c = Point::new(center.x * s, center.y * s);
                frame.fill(&canvas::Path::circle(c, radius * s), token_color(*fill));
                if *outline_width > 0.0 {
                    frame.stroke(
                        &canvas::Path::circle(c, radius * s),
                        canvas::Stroke::default()
                            .with_color(token_color(*outline))
                            .with_width(outline_width * s),
                    );
                }
            }
            RenderShape::Triangle { points, color } => {
                let path = canvas::Path::new(|b| {
                    b.move_to(Point::new(points[0].x * s, points[0].y * s));
                    b.line_to(Point::new(points[1].x * s, points[1].y * s));
                    b.line_to(Point::new(points[2].x * s, points[2].y * s));
                    b.close();
                });
                frame.fill(&path, token_color(*color));
            }
            RenderShape::Label {
                anchor,
                anchor_kind,
                content,
                px,
                bold,
                color,
            } => {
                let (align_x, align_y) = match anchor_kind {
                    TextAnchor::Center => (
                        iced::widget::text::Alignment::Center,
                        iced::alignment::Vertical::Center,
                    ),
                    TextAnchor::TopLeft => (
                        iced::widget::text::Alignment::Default,
                        iced::alignment::Vertical::Top,
                    ),
                };
                frame.fill_text(canvas::Text {
                    content: content.clone(),
                    position: Point::new(anchor.x * s, anchor.y * s),
                    color: token_color(*color),
                    size: iced::Pixels(px * s),
                    line_height: iced::widget::text::LineHeight::Relative(style::TEXT_LINE_HEIGHT),
                    font: if *bold {
                        ANNOTATION_FONT_BOLD
                    } else {
                        ANNOTATION_FONT
                    },
                    align_x,
                    align_y,
                    ..canvas::Text::default()
                });
            }
        }
    }

    fn draw_annotation(&self, frame: &mut canvas::Frame, annotation: &Annotation) {
        for shape in annotation_shapes(annotation) {
            self.draw_shape(frame, &shape);
        }
    }

    fn draft_annotation(&self) -> Option<Annotation> {
        match &self.editor.drag {
            Some(DragState::CreateNumber { tip, bubble }) => Some(Annotation::number_callout(
                AnnotationId(u64::MAX),
                self.document.next_number(),
                *tip,
                *bubble,
            )),
            Some(DragState::CreateRedaction { anchor, current }) => {
                let rect = ImageRect::from_corners(*anchor, *current);
                (!rect.is_empty())
                    .then_some(Annotation::opaque_redaction(AnnotationId(u64::MAX), rect))
            }
            Some(DragState::EditAnnotation { current, .. }) => Some(current.clone()),
            _ => None,
        }
    }

    fn draw_selection_handles(&self, frame: &mut canvas::Frame, annotation: &Annotation) {
        let s = self.scale;
        let accent = token_color(style::ACCENT);
        let white = token_color(style::WHITE);
        let handle = |frame: &mut canvas::Frame, p: ImagePoint, fill: Color, ring: Color| {
            let c = Point::new(p.x * s, p.y * s);
            frame.fill(&canvas::Path::circle(c, HANDLE_RADIUS_SCREEN), fill);
            frame.stroke(
                &canvas::Path::circle(c, HANDLE_RADIUS_SCREEN),
                canvas::Stroke::default().with_color(ring).with_width(2.0),
            );
        };
        match annotation {
            Annotation::NumberCallout { tip, bubble, .. } => {
                handle(frame, *bubble, accent, white);
                handle(frame, *tip, white, accent);
            }
            Annotation::TextNote {
                position,
                text,
                style,
                ..
            } => {
                let plate = rollshot_image_document::text_plate_rect(*position, text, *style);
                frame.stroke(
                    &canvas::Path::rectangle(
                        Point::new(plate.x * s, plate.y * s),
                        Size::new(plate.width * s, plate.height * s),
                    ),
                    canvas::Stroke::default().with_color(accent).with_width(2.0),
                );
            }
            Annotation::OpaqueRedaction { bounds, .. } => {
                for (_, p) in redaction_handles(*bounds) {
                    handle(frame, p, white, accent);
                }
            }
        }
    }
}

impl canvas::Program<Message> for AnnotationCanvas<'_> {
    type State = ();

    fn update(
        &self,
        _state: &mut (),
        event: &canvas::Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<canvas::Action<Message>> {
        match event {
            iced::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                let local = cursor.position_in(bounds)?;
                Some(
                    canvas::Action::publish(Message::CanvasPressed(self.image_point(local)))
                        .and_capture(),
                )
            }
            iced::Event::Mouse(mouse::Event::CursorMoved { .. }) => {
                let local = cursor.position_in(bounds)?;
                Some(canvas::Action::publish(Message::CanvasMoved(
                    self.image_point(local),
                )))
            }
            iced::Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                let point = release_image_point(cursor, bounds, self.scale)?;
                Some(canvas::Action::publish(Message::CanvasReleased(point)).and_capture())
            }
            _ => None,
        }
    }

    fn draw(
        &self,
        _state: &(),
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let mut frame = canvas::Frame::new(renderer, bounds.size());
        let dragged = self.dragged_id();
        let editing_text = self.editor.text_draft.as_ref().and_then(|d| d.target);

        for annotation in self.document.annotations() {
            if Some(annotation.id()) == dragged || Some(annotation.id()) == editing_text {
                continue;
            }
            if annotation_bounds(annotation).intersects(&self.visible) {
                self.draw_annotation(&mut frame, annotation);
            }
        }

        if let Some(draft) = self.draft_annotation() {
            self.draw_annotation(&mut frame, &draft);
        }

        // Smart Redaction candidate overlay. Rejected candidates are skipped.
        // Visible candidates use confidence-colored solid borders/fills and numbered
        // badges (1-based position) matching the review bar chips.
        if let Some(proposal) = self.pending_proposal {
            let review = self.review;
            let s = self.scale;
            for (index, cand) in proposal.candidates.iter().enumerate() {
                let Some(bounds) = super::workbench::proposed_edit_bounds(&cand.edit) else {
                    continue;
                };
                if !bounds.intersects(&self.visible) {
                    continue;
                }
                let is_rejected = matches!(
                    review.and_then(|r| r.per_candidate.get(&cand.id)),
                    Some(super::workbench::CandidateReviewState::Rejected)
                );
                if is_rejected {
                    continue;
                }
                let is_selected = self.selected_candidate == Some(cand.id);

                let rect = iced::Rectangle {
                    x: bounds.x * s,
                    y: bounds.y * s,
                    width: bounds.width * s,
                    height: bounds.height * s,
                };
                let style = proposal_overlay_style(cand.confidence, is_selected);
                let rect_path = canvas::Path::rectangle(
                    iced::Point::new(rect.x, rect.y),
                    iced::Size::new(rect.width, rect.height),
                );
                frame.fill(&rect_path, style.fill);
                frame.stroke(
                    &rect_path,
                    canvas::Stroke::default()
                        .with_color(style.border)
                        .with_width(style.border_width),
                );

                if s > 0.3 {
                    let sequence = index + 1;
                    let badge_center = iced::Point::new(rect.x, rect.y);
                    let badge = canvas::Path::circle(badge_center, 11.0);
                    frame.fill(&badge, style.badge);
                    frame.fill_text(canvas::Text {
                        content: sequence.to_string(),
                        position: iced::Point::new(badge_center.x - 3.5, badge_center.y + 4.0),
                        color: iced::Color::WHITE,
                        size: iced::Pixels(11.0),
                        ..canvas::Text::default()
                    });
                }
                if is_selected {
                    for handle in [
                        iced::Point::new(rect.x, rect.y),
                        iced::Point::new(rect.x + rect.width, rect.y),
                        iced::Point::new(rect.x, rect.y + rect.height),
                        iced::Point::new(rect.x + rect.width, rect.y + rect.height),
                    ] {
                        let hr = canvas::Path::rectangle(
                            handle - iced::Vector::new(3.5, 3.5),
                            iced::Size::new(7.0, 7.0),
                        );
                        frame.fill(&hr, iced::Color::from_rgb(0.13, 0.40, 1.0));
                    }
                }
            }
        }

        if let Some(id) = self.editor.selection {
            if Some(id) != dragged && Some(id) != editing_text {
                if let Some(annotation) = self.document.annotation(id) {
                    self.draw_selection_handles(&mut frame, annotation);
                }
            } else if let Some(draft) = self.draft_annotation() {
                self.draw_selection_handles(&mut frame, &draft);
            }
        }

        vec![frame.into_geometry()]
    }

    fn mouse_interaction(
        &self,
        _state: &(),
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> mouse::Interaction {
        let Some(local) = cursor.position_in(bounds) else {
            return mouse::Interaction::default();
        };
        let tolerance = HIT_TOLERANCE_SCREEN / self.scale;
        match direct_manipulation_hit(
            self.document,
            self.editor,
            self.image_point(local),
            tolerance,
        ) {
            Some(hit) => match hit.part {
                HitPart::Resize(TopLeft | BottomRight) => {
                    mouse::Interaction::ResizingDiagonallyDown
                }
                HitPart::Resize(TopRight | BottomLeft) => mouse::Interaction::ResizingDiagonallyUp,
                HitPart::Resize(Top | Bottom) => mouse::Interaction::ResizingVertically,
                HitPart::Resize(Left | Right) => mouse::Interaction::ResizingHorizontally,
                _ => mouse::Interaction::Grab,
            },
            None if self.editor.tool == Tool::Select => mouse::Interaction::default(),
            #[cfg(feature = "ocr")]
            None if self.editor.tool == Tool::OcrText => mouse::Interaction::default(),
            None => mouse::Interaction::Crosshair,
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct ProposalOverlayStyle {
    border: iced::Color,
    fill: iced::Color,
    badge: iced::Color,
    border_width: f32,
}

fn proposal_overlay_style(confidence: f32, selected: bool) -> ProposalOverlayStyle {
    let low = super::workbench::state::is_low_confidence(confidence);
    let (r, g, b) = super::workbench::state::confidence_accent(low, selected);
    let accent = iced::Color::from_rgb(r, g, b);
    let (fill, border_width) = if selected {
        (iced::Color::from_rgba(0.13, 0.40, 1.0, 0.16), 2.5)
    } else if low {
        (iced::Color::from_rgba(0.88, 0.64, 0.0, 0.20), 2.0)
    } else {
        (iced::Color::from_rgba(0.18, 0.75, 0.44, 0.18), 2.0)
    };
    ProposalOverlayStyle {
        border: accent,
        fill,
        badge: accent,
        border_width,
    }
}

/// Hit-test proposed candidates in image space. Skips rejected candidates.
pub fn hit_test_proposal_candidate(
    proposal: &rollshot_edit_proposal::EditProposal,
    point: rollshot_image_document::ImagePoint,
    review: &super::workbench::CandidateReview,
) -> Option<rollshot_edit_proposal::CandidateId> {
    use super::workbench::{proposed_edit_bounds, CandidateReviewState};
    proposal
        .candidates
        .iter()
        .find(|c| {
            if matches!(
                review.per_candidate.get(&c.id),
                Some(CandidateReviewState::Rejected)
            ) {
                return false;
            }
            proposed_edit_bounds(&c.edit).is_some_and(|b| b.contains(point))
        })
        .map(|c| c.id)
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
        let original = Annotation::number_callout(
            AnnotationId(1),
            1,
            ImagePoint::new(0.0, 0.0),
            ImagePoint::new(10.0, 10.0),
        );
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

    use image::{Rgba, RgbaImage};
    use rollshot_image_document::{annotation_bounds, ImageDocument};

    #[test]
    fn visible_image_rect_maps_scroll_and_scale() {
        let visible = visible_image_rect(
            iced::Vector::new(20.0, 40.0),
            iced::Size::new(50.0, 80.0),
            2.0,
            iced::Point::new(0.0, 0.0),
        );
        assert_eq!(
            visible,
            ImageRect {
                x: 10.0,
                y: 20.0,
                width: 25.0,
                height: 40.0
            }
        );
    }

    #[test]
    fn culling_skips_annotations_outside_the_visible_rect() {
        let mut doc = ImageDocument::new(RgbaImage::from_pixel(100, 10000, Rgba([0, 0, 0, 255])));
        let near = doc.add_number_callout(ImagePoint::new(50.0, 50.0), ImagePoint::new(50.0, 50.0));
        let far =
            doc.add_number_callout(ImagePoint::new(50.0, 9000.0), ImagePoint::new(50.0, 9000.0));
        let visible = ImageRect {
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 200.0,
        };
        let drawn: Vec<_> = doc
            .annotations()
            .iter()
            .filter(|a| annotation_bounds(a).intersects(&visible))
            .map(|a| a.id())
            .collect();
        assert_eq!(drawn, vec![near]);
        assert!(!drawn.contains(&far));
    }

    #[test]
    fn release_position_is_available_outside_canvas_bounds() {
        let bounds = Rectangle::new(Point::new(10.0, 20.0), Size::new(100.0, 200.0));
        let point = release_image_point(
            mouse::Cursor::Available(Point::new(130.0, 250.0)),
            bounds,
            2.0,
        )
        .expect("active drag release should survive leaving the canvas");
        assert_eq!(point, ImagePoint::new(60.0, 115.0));
    }

    fn assert_color_close(actual: iced::Color, expected: iced::Color) {
        assert!((actual.r - expected.r).abs() < 0.001);
        assert!((actual.g - expected.g).abs() < 0.001);
        assert!((actual.b - expected.b).abs() < 0.001);
        assert!((actual.a - expected.a).abs() < 0.001);
    }

    #[test]
    fn proposal_overlay_style_uses_green_for_high_confidence() {
        let style = proposal_overlay_style(0.92, false);

        assert_color_close(style.border, iced::Color::from_rgb(0.12, 0.55, 0.36));
        assert_color_close(style.fill, iced::Color::from_rgba(0.18, 0.75, 0.44, 0.18));
        assert_color_close(style.badge, iced::Color::from_rgb(0.12, 0.55, 0.36));
        assert_eq!(style.border_width, 2.0);
    }

    #[test]
    fn proposal_overlay_style_uses_amber_for_low_confidence() {
        let style = proposal_overlay_style(0.64, false);

        assert_color_close(style.border, iced::Color::from_rgb(0.76, 0.49, 0.04));
        assert_color_close(style.fill, iced::Color::from_rgba(0.88, 0.64, 0.0, 0.20));
        assert_color_close(style.badge, iced::Color::from_rgb(0.76, 0.49, 0.04));
    }

    #[test]
    fn proposal_overlay_style_uses_blue_for_selected_candidate() {
        let style = proposal_overlay_style(0.64, true);

        assert_color_close(style.border, iced::Color::from_rgb(0.13, 0.40, 1.0));
        assert_color_close(style.badge, iced::Color::from_rgb(0.13, 0.40, 1.0));
        assert_eq!(style.border_width, 2.5);
    }

    #[test]
    fn hit_test_proposal_candidate_finds_contained() {
        use super::super::workbench::CandidateReview;
        use rollshot_edit_proposal::{
            CandidateId, ConfidenceSummary, EditProposal, ProposalId, ProposedCandidate,
            ProposedEdit, Provenance, ProvenanceSource,
        };

        let proposal = EditProposal {
            id: ProposalId(1),
            base_document_state_id: 0,
            candidates: vec![ProposedCandidate {
                id: CandidateId(1),
                edit: ProposedEdit::AddRedaction {
                    bounds: ImageRect {
                        x: 10.0,
                        y: 10.0,
                        width: 50.0,
                        height: 50.0,
                    },
                },
                confidence: 0.9,
                label: "t".into(),
                rationale: None,
                provenance: Provenance {
                    source: ProvenanceSource::Manual,
                },
            }],
            confidence_summary: ConfidenceSummary::from_confidences(&[0.9]),
            rationale_summary: None,
            provenance: Provenance {
                source: ProvenanceSource::Manual,
            },
        };
        let review = CandidateReview::from_candidates(&[CandidateId(1)]);
        let hit = hit_test_proposal_candidate(&proposal, ImagePoint::new(20.0, 20.0), &review);
        assert_eq!(hit, Some(CandidateId(1)));
        let miss = hit_test_proposal_candidate(&proposal, ImagePoint::new(0.0, 0.0), &review);
        assert_eq!(miss, None);
    }
}
