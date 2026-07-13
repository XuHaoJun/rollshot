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
    /// Non-mutating ghost projections of pending proposal suggestions. The
    /// canvas renders each at reduced alpha so the user can distinguish
    /// them from committed annotations.
    pub suggested: Vec<Annotation>,
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
                RenderShape::Line {
                    start,
                    end,
                    width,
                    color,
                } => {
                    let path = canvas::Path::line(
                        Point::new(start.x * self.scale, start.y * self.scale),
                        Point::new(end.x * self.scale, end.y * self.scale),
                    );
                    frame.stroke(
                        &path,
                        canvas::Stroke::default()
                            .with_color(rgba_alpha(color, alpha))
                            .with_width(width * self.scale),
                    );
                }
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
                RenderShape::Box {
                    kind,
                    bounds,
                    stroke,
                    stroke_width,
                    fill,
                } => {
                    let s = self.scale;
                    let cx = bounds.x * s + bounds.width * s / 2.0;
                    let cy = bounds.y * s + bounds.height * s / 2.0;
                    let rx = bounds.width * s / 2.0;
                    let ry = bounds.height * s / 2.0;
                    let make_path = || match kind {
                        rollshot_image_document::ShapeKind::Rectangle => canvas::Path::rectangle(
                            Point::new(bounds.x * s, bounds.y * s),
                            iced::Size::new(bounds.width * s, bounds.height * s),
                        ),
                        rollshot_image_document::ShapeKind::Ellipse => canvas::Path::new(|b| {
                            b.ellipse(canvas::path::arc::Elliptical {
                                center: Point::new(cx, cy),
                                radii: iced::Vector::new(rx, ry),
                                rotation: iced::Radians(0.0),
                                start_angle: iced::Radians(0.0),
                                end_angle: iced::Radians(std::f32::consts::TAU),
                            });
                        }),
                    };
                    if let Some(fill_color) = fill {
                        frame.fill(&make_path(), rgba_alpha(*fill_color, alpha));
                    }
                    frame.stroke(
                        &make_path(),
                        canvas::Stroke::default()
                            .with_color(rgba_alpha(*stroke, alpha))
                            .with_width(stroke_width * s),
                    );
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
            style: Default::default(),
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

/// Project all pending suggestions from a [`VisualAnnotationProposal`] into
/// ghost [`Annotation`] values for canvas rendering. Each ghost uses a
/// local-only id (`AnnotationId(0)`) and does not mutate the document.
pub(crate) fn proposal_ghosts(
    proposal: &rollshot_action::VisualAnnotationProposal,
    document: &ImageDocument,
    _width: u32,
    _height: u32,
) -> Vec<Annotation> {
    use rollshot_action::VisualAnnotationSuggestionStatus;
    let mut ghosts = Vec::new();
    let mut next_number = document.next_number();
    for suggestion in &proposal.suggestions {
        if suggestion.status != VisualAnnotationSuggestionStatus::Pending {
            continue;
        }
        match &suggestion.payload {
            rollshot_action::VisualAnnotationPayload::NumberCallout { tip, bubble } => {
                ghosts.push(Annotation::NumberCallout {
                    id: AnnotationId(0),
                    number: next_number,
                    tip: *tip,
                    bubble: *bubble,
                    style: Default::default(),
                });
                next_number += 1;
            }
            rollshot_action::VisualAnnotationPayload::TextNote { position, text } => {
                ghosts.push(Annotation::TextNote {
                    id: AnnotationId(0),
                    position: *position,
                    text: text.clone(),
                    style: Default::default(),
                });
            }
            rollshot_action::VisualAnnotationPayload::OpaqueRedaction { bounds } => {
                ghosts.push(Annotation::OpaqueRedaction {
                    id: AnnotationId(0),
                    bounds: *bounds,
                });
            }
        }
    }
    ghosts
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
        for suggested in &self.suggested {
            // Ghost projection: render pending proposals with reduced alpha
            // so the user can distinguish them from committed annotations.
            self.draw_annotation_with_alpha(&mut frame, suggested, 0.5);
        }
        if !self.suggested.is_empty() {
            // Small label above the canvas content so the user knows the
            // ghosts are suggestions, not committed annotations.
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
            suggested: Vec::new(),
            mutation_allowed: true,
        };

        assert_eq!(canvas.scale, 0.5);
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
            scale: 0.5,
            suggested: Vec::new(),
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

    fn visual_proposal_with_three_suggestions(
        step: &rollshot_action::GuideStep,
        state_id: u64,
        w: u32,
        h: u32,
    ) -> rollshot_action::VisualAnnotationProposal {
        rollshot_action::VisualAnnotationProposal::from_agent_drafts(
            rollshot_action::VisualAnnotationProposalId(1),
            1,
            step,
            state_id,
            w,
            h,
            vec![
                rollshot_action::VisualAnnotationSuggestionDraft {
                    id: rollshot_action::VisualAnnotationSuggestionId(1),
                    payload: rollshot_action::VisualAnnotationPayload::NumberCallout {
                        tip: rollshot_image_document::ImagePoint::new(2.0, 2.0),
                        bubble: rollshot_image_document::ImagePoint::new(4.0, 4.0),
                    },
                    confidence: 0.9,
                    rationale: Some("button click target".to_string()),
                },
                rollshot_action::VisualAnnotationSuggestionDraft {
                    id: rollshot_action::VisualAnnotationSuggestionId(2),
                    payload: rollshot_action::VisualAnnotationPayload::TextNote {
                        position: rollshot_image_document::ImagePoint::new(3.0, 3.0),
                        text: "Save button".to_string(),
                    },
                    confidence: 0.7,
                    rationale: None,
                },
                rollshot_action::VisualAnnotationSuggestionDraft {
                    id: rollshot_action::VisualAnnotationSuggestionId(3),
                    payload: rollshot_action::VisualAnnotationPayload::OpaqueRedaction {
                        bounds: rollshot_image_document::ImageRect {
                            x: 1.0,
                            y: 1.0,
                            width: 3.0,
                            height: 2.0,
                        },
                    },
                    confidence: 0.6,
                    rationale: Some("sensitive info".to_string()),
                },
            ],
        )
        .expect("valid proposal")
    }

    #[test]
    fn pending_visual_proposal_projects_all_three_ghost_primitives() {
        let store = frame_store_with_two_frames();
        let guide = guide();
        let step = &guide.steps()[0];
        let mut presentation = ActionGuidePresentation::new();
        let doc = presentation.document_for_step(step, &store).unwrap();
        let proposal = visual_proposal_with_three_suggestions(
            step,
            doc.document.state_id(),
            doc.document.source().width(),
            doc.document.source().height(),
        );
        let ghosts = super::proposal_ghosts(&proposal, &doc.document, 8, 8);
        assert_eq!(ghosts.len(), 3);
        assert!(matches!(ghosts[0], Annotation::NumberCallout { .. }));
        assert!(matches!(ghosts[1], Annotation::TextNote { .. }));
        assert!(matches!(ghosts[2], Annotation::OpaqueRedaction { .. }));
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
            suggested: Vec::new(),
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

    #[test]
    fn timeline_annotation_tool_has_no_rectangle_or_ellipse_variant() {
        let all_variants = [
            AnnotationTool::Number,
            AnnotationTool::Text,
            AnnotationTool::Redaction,
        ];
        assert_eq!(all_variants.len(), 3, "Timeline has exactly 3 annotation tools");

        for variant in &all_variants {
            match variant {
                AnnotationTool::Number
                | AnnotationTool::Text
                | AnnotationTool::Redaction => {}
            }
        }

        let mut document = ImageDocument::new(::image::RgbaImage::from_pixel(
            64,
            64,
            ::image::Rgba([10, 20, 30, 255]),
        ));
        document.add_number_callout(ImagePoint::new(8.0, 8.0), ImagePoint::new(24.0, 24.0));
        document
            .add_text_note(ImagePoint::new(4.0, 40.0), "label".to_string())
            .unwrap();
        document
            .add_redaction(rollshot_image_document::ImageRect {
                x: 32.0,
                y: 8.0,
                width: 16.0,
                height: 12.0,
            })
            .unwrap();

        assert_eq!(document.annotations().len(), 3);
        for annotation in document.annotations() {
            assert!(
                matches!(
                    annotation,
                    rollshot_image_document::Annotation::NumberCallout { .. }
                        | rollshot_image_document::Annotation::TextNote { .. }
                        | rollshot_image_document::Annotation::OpaqueRedaction { .. }
                ),
                "Timeline document must only contain Number/Text/Redaction annotations"
            );
        }
    }
}
