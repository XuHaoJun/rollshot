use std::collections::{BTreeMap, BTreeSet};

use iced::widget::{canvas, image, text};
use iced::{alignment, mouse, Color, Point, Rectangle, Renderer, Theme};
use rollshot_action::{CandidateId, FrameId, FrameStore, GuideStep};
use rollshot_image_document::{
    annotation_shapes, Annotation, AnnotationId, ImageDocument, ImagePoint, RenderShape, Rgba8,
    TextAnchor,
};

pub(crate) struct StepAnnotationDocument {
    #[allow(dead_code)]
    pub source: CandidateId,
    pub keyframe: FrameId,
    pub document: ImageDocument,
}

#[derive(Default)]
pub(crate) struct ActionGuidePresentation {
    docs: BTreeMap<CandidateId, StepAnnotationDocument>,
}

impl ActionGuidePresentation {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn document_for_step(
        &mut self,
        step: &GuideStep,
        store: &FrameStore,
    ) -> Option<&mut StepAnnotationDocument> {
        let needs_new = self
            .docs
            .get(&step.source)
            .is_none_or(|doc| doc.keyframe != step.keyframe);
        if needs_new {
            let frame = store.retained(step.keyframe)?;
            self.docs.insert(
                step.source,
                StepAnnotationDocument {
                    source: step.source,
                    keyframe: step.keyframe,
                    document: ImageDocument::new(frame.image.clone()),
                },
            );
        }
        self.docs.get_mut(&step.source)
    }

    pub(crate) fn doc(&self, source: CandidateId) -> Option<&StepAnnotationDocument> {
        self.docs.get(&source)
    }

    #[allow(dead_code)]
    pub(crate) fn has_annotations(&self, source: CandidateId) -> bool {
        self.docs
            .get(&source)
            .is_some_and(|doc| !doc.document.annotations().is_empty())
    }

    #[allow(dead_code)]
    pub(crate) fn clear_for_source(&mut self, source: CandidateId) -> bool {
        self.docs.remove(&source).is_some()
    }

    #[allow(dead_code)]
    pub(crate) fn retain_sources(&mut self, sources: impl IntoIterator<Item = CandidateId>) {
        let keep: BTreeSet<_> = sources.into_iter().collect();
        self.docs.retain(|source, _| keep.contains(source));
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AnnotationTool {
    Number,
    Text,
    Redaction,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum AnnotationDraft {
    Number {
        tip: ImagePoint,
        bubble: ImagePoint,
    },
    Redaction {
        start: ImagePoint,
        current: ImagePoint,
    },
}

impl AnnotationDraft {
    pub(crate) fn redaction_rect(&self) -> Option<rollshot_image_document::ImageRect> {
        match self {
            AnnotationDraft::Redaction { start, current } => Some(
                rollshot_image_document::ImageRect::from_corners(*start, *current),
            ),
            AnnotationDraft::Number { .. } => None,
        }
    }
}

pub(crate) struct StepAnnotationSession {
    pub source: CandidateId,
    #[allow(dead_code)]
    pub keyframe: FrameId,
    pub handle: image::Handle,
    pub width: u32,
    pub height: u32,
    pub tool: AnnotationTool,
    pub text_note: String,
    pub draft: Option<AnnotationDraft>,
}

impl StepAnnotationSession {
    pub(crate) fn new(source: CandidateId, keyframe: FrameId, image: &::image::RgbaImage) -> Self {
        Self {
            source,
            keyframe,
            handle: super::build_handle(image),
            width: image.width(),
            height: image.height(),
            tool: AnnotationTool::Number,
            text_note: String::new(),
            draft: None,
        }
    }
}

pub(crate) struct NumberAnnotationCanvas<'a> {
    pub document: &'a ImageDocument,
    pub draft: Option<AnnotationDraft>,
    pub scale: f32,
    /// Optional non-mutating ghost projection of a pending callout
    /// proposal. The canvas renders it with reduced alpha and a small
    /// `Suggested` label so the user can review before accepting.
    pub suggested: Option<Annotation>,
    /// When `false`, the canvas does not publish pointer events; manual
    /// annotation tools and canvas mutation are suspended.
    pub mutation_allowed: bool,
}

impl NumberAnnotationCanvas<'_> {
    fn image_point(&self, local: Point) -> ImagePoint {
        ImagePoint::new(local.x / self.scale, local.y / self.scale)
    }

    fn draw_annotation(&self, frame: &mut canvas::Frame, annotation: &Annotation) {
        self.draw_annotation_with_alpha(frame, annotation, 1.0);
    }

    fn draw_annotation_with_alpha(
        &self,
        frame: &mut canvas::Frame,
        annotation: &Annotation,
        alpha: f32,
    ) {
        for shape in annotation_shapes(annotation) {
            match shape {
                RenderShape::Circle {
                    center,
                    radius,
                    fill,
                    outline_width,
                    outline,
                } => {
                    let path = canvas::Path::circle(
                        Point::new(center.x * self.scale, center.y * self.scale),
                        radius * self.scale,
                    );
                    frame.fill(&path, rgba_alpha(fill, alpha));
                    frame.stroke(
                        &path,
                        canvas::Stroke::default()
                            .with_color(rgba_alpha(outline, alpha))
                            .with_width(outline_width * self.scale),
                    );
                }
                RenderShape::Triangle { points, color } => {
                    let path = canvas::Path::new(|builder| {
                        builder.move_to(Point::new(
                            points[0].x * self.scale,
                            points[0].y * self.scale,
                        ));
                        builder.line_to(Point::new(
                            points[1].x * self.scale,
                            points[1].y * self.scale,
                        ));
                        builder.line_to(Point::new(
                            points[2].x * self.scale,
                            points[2].y * self.scale,
                        ));
                        builder.close();
                    });
                    frame.fill(&path, rgba_alpha(color, alpha));
                }
                RenderShape::Label {
                    anchor,
                    anchor_kind: TextAnchor::Center,
                    content,
                    px,
                    bold,
                    color,
                } => {
                    frame.fill_text(canvas::Text {
                        content,
                        position: Point::new(anchor.x * self.scale, anchor.y * self.scale),
                        color: rgba_alpha(color, alpha),
                        size: iced::Pixels(px * self.scale),
                        align_x: text::Alignment::Center,
                        align_y: alignment::Vertical::Center,
                        font: if bold {
                            iced::Font {
                                weight: iced::font::Weight::Bold,
                                ..iced::Font::with_name(
                                    rollshot_image_document::style::FONT_FAMILY_NAME,
                                )
                            }
                        } else {
                            iced::Font::with_name(rollshot_image_document::style::FONT_FAMILY_NAME)
                        },
                        ..canvas::Text::default()
                    });
                }
                RenderShape::Rect { rect, color } => {
                    let path = canvas::Path::rectangle(
                        Point::new(rect.x * self.scale, rect.y * self.scale),
                        iced::Size::new(rect.width * self.scale, rect.height * self.scale),
                    );
                    frame.fill(&path, rgba_alpha(color, alpha));
                }
                RenderShape::Label {
                    anchor,
                    anchor_kind: TextAnchor::TopLeft,
                    content,
                    px,
                    bold,
                    color,
                } => {
                    frame.fill_text(canvas::Text {
                        content,
                        position: Point::new(anchor.x * self.scale, anchor.y * self.scale),
                        color: rgba_alpha(color, alpha),
                        size: iced::Pixels(px * self.scale),
                        align_x: text::Alignment::Left,
                        align_y: alignment::Vertical::Top,
                        font: if bold {
                            iced::Font {
                                weight: iced::font::Weight::Bold,
                                ..iced::Font::with_name(
                                    rollshot_image_document::style::FONT_FAMILY_NAME,
                                )
                            }
                        } else {
                            iced::Font::with_name(rollshot_image_document::style::FONT_FAMILY_NAME)
                        },
                        ..canvas::Text::default()
                    });
                }
            }
        }
    }
}

fn rgba_alpha(c: Rgba8, alpha: f32) -> Color {
    Color::from_rgba8(c.r, c.g, c.b, (c.a as f32 / 255.0) * alpha)
}

fn draft_annotation(document: &ImageDocument, draft: AnnotationDraft) -> Option<Annotation> {
    match draft {
        AnnotationDraft::Number { tip, bubble } => Some(Annotation::NumberCallout {
            id: AnnotationId(0),
            number: document.annotations().len() as u32 + 1,
            tip,
            bubble,
        }),
        AnnotationDraft::Redaction { .. } => {
            draft
                .redaction_rect()
                .map(|bounds| Annotation::OpaqueRedaction {
                    id: AnnotationId(0),
                    bounds,
                })
        }
    }
}

/// Project a pending callout proposal into a temporary [`Annotation`] for
/// ghost rendering. The helper is pure projection only: it does not call
/// `add_number_callout` and never mutates the document's `state_id`,
/// annotations, undo, or redo stacks. The returned annotation uses the
/// number the document would assign on accept and the deterministic
/// bubble placed by [`rollshot_image_document::place_number_callout_bubble`].
pub(crate) fn suggested_callout_annotation(
    document: &ImageDocument,
    suggestion: &rollshot_action::CalloutSuggestion,
    width: u32,
    height: u32,
) -> Annotation {
    let bubble = rollshot_image_document::place_number_callout_bubble(
        suggestion.tip,
        width,
        height,
        document.annotations(),
    );
    Annotation::NumberCallout {
        id: AnnotationId(0),
        number: document.next_number(),
        tip: suggestion.tip,
        bubble,
    }
}

impl canvas::Program<super::Message> for NumberAnnotationCanvas<'_> {
    type State = ();

    fn update(
        &self,
        _state: &mut (),
        event: &canvas::Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<canvas::Action<super::Message>> {
        if !self.mutation_allowed {
            return None;
        }
        match event {
            iced::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                let local = cursor.position_in(bounds)?;
                Some(
                    canvas::Action::publish(super::Message::AnnotationCanvasPressed(
                        self.image_point(local),
                    ))
                    .and_capture(),
                )
            }
            iced::Event::Mouse(mouse::Event::CursorMoved { .. }) => {
                let local = cursor.position_in(bounds)?;
                Some(canvas::Action::publish(
                    super::Message::AnnotationCanvasMoved(self.image_point(local)),
                ))
            }
            iced::Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                let local = cursor.position_in(bounds)?;
                Some(
                    canvas::Action::publish(super::Message::AnnotationCanvasReleased(
                        self.image_point(local),
                    ))
                    .and_capture(),
                )
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
        for annotation in self.document.annotations() {
            self.draw_annotation(&mut frame, annotation);
        }
        if let Some(draft) = self
            .draft
            .and_then(|draft| draft_annotation(self.document, draft))
        {
            self.draw_annotation(&mut frame, &draft);
        }
        if let Some(suggested) = &self.suggested {
            // Ghost projection: render the pending proposal with reduced alpha
            // so the user can distinguish it from a committed annotation.
            self.draw_annotation_with_alpha(&mut frame, suggested, 0.5);
            // Small label above the canvas content so the user knows the
            // ghost is a suggestion, not a committed annotation.
            frame.fill_text(canvas::Text {
                content: "Suggested".to_string(),
                position: Point::new(8.0, 8.0),
                color: Color::from_rgba(1.0, 1.0, 1.0, 0.9),
                size: iced::Pixels(12.0),
                align_x: text::Alignment::Left,
                align_y: alignment::Vertical::Top,
                font: iced::Font::with_name(rollshot_image_document::style::FONT_FAMILY_NAME),
                ..canvas::Text::default()
            });
        }
        vec![frame.into_geometry()]
    }

    fn mouse_interaction(
        &self,
        _state: &(),
        _bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> mouse::Interaction {
        if self.mutation_allowed {
            mouse::Interaction::Crosshair
        } else {
            mouse::Interaction::Idle
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rollshot_action::{
        CandidateKind, CandidateStep, DetectReason, FrameStore, Guide, StoreConfig,
    };

    fn frame_store_with_two_frames() -> FrameStore {
        let mut store = FrameStore::new(StoreConfig::default());
        let first = store.ingest(
            ::image::RgbaImage::from_pixel(8, 8, ::image::Rgba([0, 0, 0, 255])),
            0,
        );
        let second = store.ingest(
            ::image::RgbaImage::from_pixel(8, 8, ::image::Rgba([255, 255, 255, 255])),
            100,
        );
        store.retain_window(first);
        store.retain_window(second);
        store
    }

    fn guide() -> Guide {
        Guide::from_candidates(vec![CandidateStep {
            id: 42,
            kind: CandidateKind::Click,
            reason: DetectReason::VisualChange,
            at_ms: 100,
            keyframe: 0,
            nearby: vec![0, 1],
        }])
    }

    #[test]
    fn document_for_step_is_keyed_by_source_and_uses_current_keyframe() {
        let store = frame_store_with_two_frames();
        let guide = guide();
        let step = &guide.steps()[0];
        let mut presentation = ActionGuidePresentation::new();

        let doc = presentation
            .document_for_step(step, &store)
            .expect("document exists");

        assert_eq!(doc.source, 42);
        assert_eq!(doc.keyframe, 0);
        assert_eq!(doc.document.source().dimensions(), (8, 8));
        assert!(!presentation.has_annotations(step.source));
    }

    #[test]
    fn clear_for_keyframe_change_removes_only_matching_step() {
        let store = frame_store_with_two_frames();
        let guide = guide();
        let step = &guide.steps()[0];
        let mut presentation = ActionGuidePresentation::new();
        let doc = presentation.document_for_step(step, &store).unwrap();
        doc.document.add_number_callout(
            rollshot_image_document::ImagePoint::new(1.0, 1.0),
            rollshot_image_document::ImagePoint::new(4.0, 4.0),
        );

        assert!(presentation.clear_for_source(step.source));
        assert!(!presentation.has_annotations(step.source));
        assert!(!presentation.clear_for_source(step.source));
    }

    #[test]
    fn retain_sources_prunes_deleted_steps() {
        let store = frame_store_with_two_frames();
        let guide = guide();
        let step = &guide.steps()[0];
        let mut presentation = ActionGuidePresentation::new();
        presentation.document_for_step(step, &store).unwrap();

        presentation.retain_sources(std::iter::empty());

        assert!(!presentation.has_annotations(step.source));
    }

    #[test]
    fn annotation_session_defaults_to_number_tool_with_empty_text() {
        let image = ::image::RgbaImage::from_pixel(16, 12, ::image::Rgba([0, 0, 0, 255]));
        let session = StepAnnotationSession::new(7, 3, &image);

        assert_eq!(session.tool, AnnotationTool::Number);
        assert_eq!(session.text_note, "");
        assert_eq!(session.width, 16);
        assert_eq!(session.height, 12);
        assert!(session.draft.is_none());
    }

    #[test]
    fn redaction_draft_rect_normalizes_drag_direction() {
        let draft = AnnotationDraft::Redaction {
            start: ImagePoint::new(12.0, 9.0),
            current: ImagePoint::new(2.0, 3.0),
        };

        assert_eq!(
            draft.redaction_rect(),
            Some(rollshot_image_document::ImageRect {
                x: 2.0,
                y: 3.0,
                width: 10.0,
                height: 6.0,
            })
        );
    }

    #[test]
    fn draft_annotation_converts_redaction_draft_to_opaque_redaction() {
        let document = ImageDocument::new(::image::RgbaImage::from_pixel(
            64,
            64,
            ::image::Rgba([10, 20, 30, 255]),
        ));
        let annotation = draft_annotation(
            &document,
            AnnotationDraft::Redaction {
                start: ImagePoint::new(12.0, 9.0),
                current: ImagePoint::new(2.0, 3.0),
            },
        )
        .expect("draft annotation");

        assert!(matches!(
            annotation,
            Annotation::OpaqueRedaction { bounds, .. }
                if bounds
                    == (rollshot_image_document::ImageRect {
                        x: 2.0,
                        y: 3.0,
                        width: 10.0,
                        height: 6.0,
                    })
        ));
    }

    #[test]
    fn number_annotation_canvas_accepts_mixed_annotations_and_redaction_draft() {
        let mut document = ImageDocument::new(::image::RgbaImage::from_pixel(
            64,
            64,
            ::image::Rgba([10, 20, 30, 255]),
        ));
        document.add_number_callout(ImagePoint::new(8.0, 8.0), ImagePoint::new(24.0, 24.0));
        document
            .add_text_note(ImagePoint::new(4.0, 40.0), "Check this label".to_string())
            .unwrap();
        document
            .add_redaction(rollshot_image_document::ImageRect {
                x: 32.0,
                y: 8.0,
                width: 16.0,
                height: 12.0,
            })
            .unwrap();

        let canvas = NumberAnnotationCanvas {
            document: &document,
            draft: Some(AnnotationDraft::Redaction {
                start: ImagePoint::new(1.0, 1.0),
                current: ImagePoint::new(12.0, 10.0),
            }),
            scale: 0.5,
            suggested: None,
            mutation_allowed: true,
        };

        assert_eq!(canvas.scale, 0.5);
    }

    fn suggestion(tip: ImagePoint, width: u32, height: u32) -> rollshot_action::CalloutSuggestion {
        rollshot_action::CalloutSuggestion {
            id: rollshot_action::CalloutSuggestionId(1),
            base: rollshot_action::CalloutSuggestionBase {
                step_source: 42,
                keyframe: 0,
                document_state_id: 0,
                image_width: width,
                image_height: height,
            },
            tip,
            confidence: 0.75,
            rationale: Some("test".to_string()),
            provenance: rollshot_action::CalloutProposalProvenance::Agent { run_id: 1 },
            status: rollshot_action::CalloutSuggestionStatus::Pending,
        }
    }

    #[test]
    fn suggested_callout_annotation_uses_next_number_from_document() {
        let mut document = ImageDocument::new(::image::RgbaImage::from_pixel(
            64,
            48,
            ::image::Rgba([0, 0, 0, 255]),
        ));
        document.add_number_callout(ImagePoint::new(4.0, 4.0), ImagePoint::new(12.0, 12.0));
        let expected_number = document.next_number();
        let suggestion = suggestion(ImagePoint::new(20.0, 20.0), 64, 48);

        let annotation = super::suggested_callout_annotation(&document, &suggestion, 64, 48);

        match annotation {
            Annotation::NumberCallout { number, .. } => {
                assert_eq!(number, expected_number);
            }
            other => panic!("expected NumberCallout, got {other:?}"),
        }
    }

    #[test]
    fn suggested_callout_annotation_preserves_tip_from_suggestion() {
        let document = ImageDocument::new(::image::RgbaImage::from_pixel(
            64,
            48,
            ::image::Rgba([0, 0, 0, 255]),
        ));
        let tip = ImagePoint::new(20.0, 30.0);
        let suggestion = suggestion(tip, 64, 48);

        let annotation = super::suggested_callout_annotation(&document, &suggestion, 64, 48);

        match annotation {
            Annotation::NumberCallout {
                tip: returned_tip, ..
            } => assert_eq!(returned_tip, tip),
            other => panic!("expected NumberCallout, got {other:?}"),
        }
    }

    #[test]
    fn suggested_callout_annotation_uses_placement_bubble() {
        let document = ImageDocument::new(::image::RgbaImage::from_pixel(
            64,
            48,
            ::image::Rgba([0, 0, 0, 255]),
        ));
        let tip = ImagePoint::new(20.0, 20.0);
        let suggestion = suggestion(tip, 64, 48);
        let expected_bubble = rollshot_image_document::place_number_callout_bubble(
            tip,
            64,
            48,
            document.annotations(),
        );

        let annotation = super::suggested_callout_annotation(&document, &suggestion, 64, 48);

        match annotation {
            Annotation::NumberCallout { bubble, .. } => {
                assert_eq!(bubble, expected_bubble);
            }
            other => panic!("expected NumberCallout, got {other:?}"),
        }
    }

    #[test]
    fn suggested_callout_annotation_does_not_mutate_document() {
        let mut document = ImageDocument::new(::image::RgbaImage::from_pixel(
            64,
            48,
            ::image::Rgba([0, 0, 0, 255]),
        ));
        document.add_number_callout(ImagePoint::new(4.0, 4.0), ImagePoint::new(12.0, 12.0));
        let baseline_state_id = document.state_id();
        let baseline_annotations = document.annotations().to_vec();
        let baseline_undo = document.can_undo();
        let baseline_redo = document.can_redo();
        let baseline_next_number = document.next_number();
        let suggestion = suggestion(ImagePoint::new(20.0, 20.0), 64, 48);

        let _ = super::suggested_callout_annotation(&document, &suggestion, 64, 48);

        assert_eq!(document.state_id(), baseline_state_id);
        assert_eq!(document.annotations(), baseline_annotations);
        assert_eq!(document.can_undo(), baseline_undo);
        assert_eq!(document.can_redo(), baseline_redo);
        assert_eq!(document.next_number(), baseline_next_number);
    }

    #[test]
    fn number_annotation_canvas_carries_suggested_and_mutation_allowed() {
        let document = ImageDocument::new(::image::RgbaImage::from_pixel(
            64,
            48,
            ::image::Rgba([0, 0, 0, 255]),
        ));
        let suggestion = suggestion(ImagePoint::new(20.0, 20.0), 64, 48);
        let projected = super::suggested_callout_annotation(&document, &suggestion, 64, 48);

        let canvas = NumberAnnotationCanvas {
            document: &document,
            draft: None,
            scale: 0.5,
            suggested: Some(projected),
            mutation_allowed: false,
        };

        assert!(
            canvas.suggested.is_some(),
            "suggested ghost should be present"
        );
        assert!(!canvas.mutation_allowed, "mutation should be disabled");
    }

    #[test]
    fn number_annotation_canvas_skips_event_publication_when_mutation_disabled() {
        use iced::mouse;
        let document = ImageDocument::new(::image::RgbaImage::from_pixel(
            64,
            48,
            ::image::Rgba([0, 0, 0, 255]),
        ));
        let canvas = NumberAnnotationCanvas {
            document: &document,
            draft: None,
            scale: 1.0,
            suggested: None,
            mutation_allowed: false,
        };
        let bounds = Rectangle {
            x: 0.0,
            y: 0.0,
            width: 64.0,
            height: 48.0,
        };
        let cursor = mouse::Cursor::Available(Point::new(8.0, 8.0));

        let action = <NumberAnnotationCanvas<'_> as canvas::Program<super::super::Message>>::update(
            &canvas,
            &mut (),
            &canvas::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
            bounds,
            cursor,
        );

        assert!(
            action.is_none(),
            "mutation-disabled canvas must not publish pointer events"
        );
    }

    #[test]
    fn number_annotation_canvas_publishes_events_when_mutation_allowed() {
        use iced::mouse;
        let document = ImageDocument::new(::image::RgbaImage::from_pixel(
            64,
            48,
            ::image::Rgba([0, 0, 0, 255]),
        ));
        let canvas = NumberAnnotationCanvas {
            document: &document,
            draft: None,
            scale: 1.0,
            suggested: None,
            mutation_allowed: true,
        };
        let bounds = Rectangle {
            x: 0.0,
            y: 0.0,
            width: 64.0,
            height: 48.0,
        };
        let cursor = mouse::Cursor::Available(Point::new(8.0, 8.0));

        let action = <NumberAnnotationCanvas<'_> as canvas::Program<super::super::Message>>::update(
            &canvas,
            &mut (),
            &canvas::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
            bounds,
            cursor,
        );

        assert!(
            action.is_some(),
            "mutation-allowed canvas should publish pointer events"
        );
    }
}
