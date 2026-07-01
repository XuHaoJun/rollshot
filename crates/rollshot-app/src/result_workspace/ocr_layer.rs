use iced::advanced::layout::{self, Layout};
use iced::advanced::renderer;
use iced::advanced::text::{self, Paragraph};
use iced::advanced::widget::{self, tree, Widget};
use iced::advanced::{mouse, Clipboard, Shell};
use iced::{alignment, Color, Element, Event, Length, Point, Rectangle, Size};
use rollshot_image_document::{ImagePoint, ImageRect};

use crate::diagnostics::TARGET_OCR_TEXT;

use super::ocr_text::{OcrSelection, OcrTextDocument, TextCursor};
use super::update::Message;

pub struct OcrTextLayer<'a> {
    document: Option<&'a OcrTextDocument>,
    selection: Option<OcrSelection>,
    scale: f32,
    visible: ImageRect,
    width: f32,
    height: f32,
}

struct State<Renderer>
where
    Renderer: text::Renderer,
{
    paragraphs: Vec<(usize, Renderer::Paragraph)>,
    dragging: bool,
}

impl<Renderer> Default for State<Renderer>
where
    Renderer: text::Renderer,
{
    fn default() -> Self {
        Self {
            paragraphs: Vec::new(),
            dragging: false,
        }
    }
}

pub fn ocr_text_layer(
    document: Option<&OcrTextDocument>,
    selection: Option<OcrSelection>,
    scale: f32,
    visible: ImageRect,
    size: Size,
) -> OcrTextLayer<'_> {
    OcrTextLayer {
        document,
        selection,
        scale,
        visible,
        width: size.width,
        height: size.height,
    }
}

impl<'a, MessageT, Theme, Renderer> Widget<MessageT, Theme, Renderer> for OcrTextLayer<'a>
where
    MessageT: From<Message> + Clone + 'a,
    Renderer: renderer::Renderer + text::Renderer<Font = iced::Font> + 'static,
{
    fn size(&self) -> Size<Length> {
        Size {
            width: Length::Fixed(self.width),
            height: Length::Fixed(self.height),
        }
    }

    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<State<Renderer>>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(State::<Renderer>::default())
    }

    fn layout(
        &mut self,
        tree: &mut widget::Tree,
        renderer: &Renderer,
        _limits: &layout::Limits,
    ) -> layout::Node {
        let state = tree.state.downcast_mut::<State<Renderer>>();
        state.paragraphs.clear();
        if let Some(document) = self.document {
            for (index, item) in document.visible_items().iter().enumerate() {
                if !item.bounds.intersects(&self.visible) {
                    continue;
                }
                state.paragraphs.push((
                    index,
                    Renderer::Paragraph::with_text(text::Text {
                        content: item.text.as_str(),
                        bounds: Size::new(
                            item.bounds.width * self.scale,
                            item.bounds.height * self.scale,
                        ),
                        size: iced::Pixels((item.bounds.height * self.scale).max(8.0)),
                        line_height: text::LineHeight::Relative(1.0),
                        font: renderer.default_font(),
                        align_x: text::Alignment::Left,
                        align_y: alignment::Vertical::Center,
                        shaping: text::Shaping::Advanced,
                        wrapping: text::Wrapping::None,
                    }),
                ));
            }
            tracing::debug!(
                target: TARGET_OCR_TEXT,
                scale = self.scale,
                layer_width = self.width,
                layer_height = self.height,
                visible_x = self.visible.x,
                visible_y = self.visible.y,
                visible_width = self.visible.width,
                visible_height = self.visible.height,
                document_items = document.visible_items().len(),
                laid_out_items = state.paragraphs.len(),
                "ocr layer layout"
            );
        }
        layout::Node::new(Size::new(self.width, self.height))
    }

    fn update(
        &mut self,
        tree: &mut widget::Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        _renderer: &Renderer,
        _clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, MessageT>,
        _viewport: &Rectangle,
    ) {
        let Some(document) = self.document else {
            return;
        };
        let state = tree.state.downcast_mut::<State<Renderer>>();

        match event {
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                let bounds = layout.bounds();
                let Some(point) = image_point_from_cursor(cursor, bounds, self.scale) else {
                    log_cursor_mapping_miss("press", cursor, bounds, self.scale, self.visible);
                    return;
                };
                if let Some(hit) = hit_test(
                    document,
                    &state.paragraphs,
                    self.scale,
                    point,
                    Some("press"),
                ) {
                    log_cursor_mapping(
                        "press",
                        cursor,
                        bounds,
                        self.scale,
                        self.visible,
                        point,
                        Some(hit),
                    );
                    state.dragging = true;
                    shell.publish(Message::OcrSelectionStarted(hit).into());
                    shell.capture_event();
                } else {
                    log_cursor_mapping(
                        "press",
                        cursor,
                        bounds,
                        self.scale,
                        self.visible,
                        point,
                        None,
                    );
                }
            }
            Event::Mouse(mouse::Event::CursorMoved { .. }) => {
                if !state.dragging {
                    return;
                }
                let bounds = layout.bounds();
                let Some(point) = image_point_from_cursor(cursor, bounds, self.scale) else {
                    log_cursor_mapping_miss("drag", cursor, bounds, self.scale, self.visible);
                    shell.capture_event();
                    return;
                };
                if let Some(hit) =
                    hit_test(document, &state.paragraphs, self.scale, point, Some("drag"))
                {
                    log_cursor_mapping(
                        "drag",
                        cursor,
                        bounds,
                        self.scale,
                        self.visible,
                        point,
                        Some(hit),
                    );
                    shell.publish(Message::OcrSelectionChanged(hit).into());
                } else {
                    log_cursor_mapping(
                        "drag",
                        cursor,
                        bounds,
                        self.scale,
                        self.visible,
                        point,
                        None,
                    );
                }
                shell.capture_event();
            }
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                if !state.dragging {
                    return;
                }
                state.dragging = false;
                let bounds = layout.bounds();
                let Some(point) = image_point_from_cursor(cursor, bounds, self.scale) else {
                    log_cursor_mapping_miss("release", cursor, bounds, self.scale, self.visible);
                    shell.capture_event();
                    return;
                };
                if let Some(hit) = hit_test(
                    document,
                    &state.paragraphs,
                    self.scale,
                    point,
                    Some("release"),
                ) {
                    log_cursor_mapping(
                        "release",
                        cursor,
                        bounds,
                        self.scale,
                        self.visible,
                        point,
                        Some(hit),
                    );
                    shell.publish(Message::OcrSelectionFinished(hit).into());
                } else {
                    log_cursor_mapping(
                        "release",
                        cursor,
                        bounds,
                        self.scale,
                        self.visible,
                        point,
                        None,
                    );
                }
                shell.capture_event();
            }
            _ => {}
        }
    }

    fn draw(
        &self,
        tree: &widget::Tree,
        renderer: &mut Renderer,
        _theme: &Theme,
        _style: &renderer::Style,
        layout: Layout<'_>,
        _cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        let Some(document) = self.document else {
            return;
        };
        let state = tree.state.downcast_ref::<State<Renderer>>();
        let origin = layout.bounds().position();

        draw_selection(
            renderer,
            document,
            self.selection,
            self.scale,
            origin,
            &state.paragraphs,
        );

        for (index, paragraph) in &state.paragraphs {
            let item = &document.visible_items()[*index];
            let position = Point::new(
                origin.x + item.bounds.x * self.scale,
                origin.y + item.bounds.y * self.scale,
            );
            renderer.fill_paragraph(
                paragraph,
                position,
                Color {
                    r: 0.05,
                    g: 0.05,
                    b: 0.05,
                    a: 0.01,
                },
                *viewport,
            );
        }
    }

    fn mouse_interaction(
        &self,
        tree: &widget::Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        _viewport: &Rectangle,
        _renderer: &Renderer,
    ) -> mouse::Interaction {
        let Some(document) = self.document else {
            return mouse::Interaction::default();
        };
        let Some(point) = image_point_from_cursor(cursor, layout.bounds(), self.scale) else {
            return mouse::Interaction::default();
        };
        let state = tree.state.downcast_ref::<State<Renderer>>();
        if hit_test(document, &state.paragraphs, self.scale, point, None).is_some() {
            mouse::Interaction::Text
        } else {
            mouse::Interaction::default()
        }
    }
}

impl<'a, MessageT, Theme, Renderer> From<OcrTextLayer<'a>>
    for Element<'a, MessageT, Theme, Renderer>
where
    MessageT: From<Message> + Clone + 'a,
    Renderer: renderer::Renderer + text::Renderer<Font = iced::Font> + 'static + 'a,
    Theme: 'a,
{
    fn from(layer: OcrTextLayer<'a>) -> Self {
        Element::new(layer)
    }
}

fn image_point_from_cursor(
    cursor: mouse::Cursor,
    bounds: Rectangle,
    scale: f32,
) -> Option<ImagePoint> {
    let local = cursor.position_in(bounds)?;
    Some(ImagePoint {
        x: local.x / scale,
        y: local.y / scale,
    })
}

fn log_cursor_mapping(
    phase: &'static str,
    cursor: mouse::Cursor,
    bounds: Rectangle,
    scale: f32,
    visible: ImageRect,
    point: ImagePoint,
    hit: Option<TextCursor>,
) {
    let screen = cursor.position();
    let local = cursor.position_in(bounds);
    tracing::debug!(
        target: TARGET_OCR_TEXT,
        phase,
        scale,
        screen_x = screen.map(|p| p.x),
        screen_y = screen.map(|p| p.y),
        layer_x = bounds.x,
        layer_y = bounds.y,
        layer_width = bounds.width,
        layer_height = bounds.height,
        local_x = local.map(|p| p.x),
        local_y = local.map(|p| p.y),
        image_x = point.x,
        image_y = point.y,
        visible_x = visible.x,
        visible_y = visible.y,
        visible_width = visible.width,
        visible_height = visible.height,
        hit_item_index = hit.map(|cursor| cursor.item_index),
        hit_char_index = hit.map(|cursor| cursor.char_index),
        "ocr cursor mapping"
    );
}

fn log_cursor_mapping_miss(
    phase: &'static str,
    cursor: mouse::Cursor,
    bounds: Rectangle,
    scale: f32,
    visible: ImageRect,
) {
    let screen = cursor.position();
    tracing::debug!(
        target: TARGET_OCR_TEXT,
        phase,
        scale,
        screen_x = screen.map(|p| p.x),
        screen_y = screen.map(|p| p.y),
        layer_x = bounds.x,
        layer_y = bounds.y,
        layer_width = bounds.width,
        layer_height = bounds.height,
        visible_x = visible.x,
        visible_y = visible.y,
        visible_width = visible.width,
        visible_height = visible.height,
        "ocr cursor outside layer"
    );
}

fn hit_test<ParagraphT>(
    document: &OcrTextDocument,
    paragraphs: &[(usize, ParagraphT)],
    scale: f32,
    point: ImagePoint,
    phase: Option<&'static str>,
) -> Option<TextCursor>
where
    ParagraphT: Paragraph<Font = iced::Font>,
{
    for (index, paragraph) in paragraphs {
        let item = &document.visible_items()[*index];
        if !item.bounds.contains(point) {
            continue;
        }
        let local = Point::new(
            (point.x - item.bounds.x) * scale,
            (point.y - item.bounds.y) * scale,
        );
        let char_index = paragraph
            .hit_test(local)
            .map(|hit| super::ocr_text::char_index_for_byte_offset(&item.text, hit.cursor()))
            .unwrap_or_else(|| super::ocr_text::character_index_for_axis_aligned_item(item, point));
        if let Some(phase) = phase {
            tracing::trace!(
                target: TARGET_OCR_TEXT,
                phase,
                item_index = *index,
                text_chars = item.text.chars().count(),
                item_x = item.bounds.x,
                item_y = item.bounds.y,
                item_width = item.bounds.width,
                item_height = item.bounds.height,
                point_x = point.x,
                point_y = point.y,
                paragraph_local_x = local.x,
                paragraph_local_y = local.y,
                char_index,
                "ocr hit test matched item"
            );
        }
        return Some(TextCursor::new(*index, char_index));
    }
    if let Some(phase) = phase {
        tracing::trace!(
            target: TARGET_OCR_TEXT,
            phase,
            point_x = point.x,
            point_y = point.y,
            paragraphs = paragraphs.len(),
            "ocr hit test missed"
        );
    }
    None
}

#[derive(Debug, Clone, Copy)]
struct SelectionHighlight {
    bounds: Rectangle,
    start_char: usize,
    end_char: usize,
    start_frac: f32,
    end_frac: f32,
    paragraph_start_x: f32,
    paragraph_end_x: f32,
    fallback_start_x: f32,
    fallback_end_x: f32,
}

fn selection_highlight(
    item_index: usize,
    item: &super::ocr_text::OcrTextItem,
    start: TextCursor,
    end: TextCursor,
    scale: f32,
    origin: Point,
    grapheme_x: impl Fn(usize) -> Option<f32>,
) -> Option<SelectionHighlight> {
    if item_index < start.item_index || item_index > end.item_index {
        return None;
    }

    let chars = item.text.chars().count().max(1);
    let start_char = if item_index == start.item_index {
        start.char_index
    } else {
        0
    };
    let end_char = if item_index == end.item_index {
        end.char_index
    } else {
        item.text.chars().count()
    };
    let start_frac = start_char as f32 / chars as f32;
    let end_frac = end_char as f32 / chars as f32;
    if end_frac <= start_frac {
        return None;
    }

    let fallback_start_x = item.bounds.width * start_frac * scale;
    let fallback_end_x = item.bounds.width * end_frac * scale;
    let paragraph_start_x = grapheme_x(start_char).unwrap_or(fallback_start_x);
    let paragraph_end_x = grapheme_x(end_char).unwrap_or(fallback_end_x);
    let width = (paragraph_end_x - paragraph_start_x).max(0.0);
    if width <= 0.0 {
        return None;
    }

    Some(SelectionHighlight {
        bounds: Rectangle {
            x: origin.x + item.bounds.x * scale + paragraph_start_x,
            y: origin.y + item.bounds.y * scale,
            width,
            height: item.bounds.height * scale,
        },
        start_char,
        end_char,
        start_frac,
        end_frac,
        paragraph_start_x,
        paragraph_end_x,
        fallback_start_x,
        fallback_end_x,
    })
}

fn draw_selection<Renderer>(
    renderer: &mut Renderer,
    document: &OcrTextDocument,
    selection: Option<OcrSelection>,
    scale: f32,
    origin: Point,
    paragraphs: &[(usize, Renderer::Paragraph)],
) where
    Renderer: renderer::Renderer + text::Renderer<Font = iced::Font>,
{
    let Some(selection) = selection else {
        return;
    };
    let (start, end) = selection.normalized();
    let color = Color {
        r: 0.10,
        g: 0.42,
        b: 0.95,
        a: 0.28,
    };
    for (index, paragraph) in paragraphs {
        let item = &document.visible_items()[*index];
        let Some(highlight) = selection_highlight(*index, item, start, end, scale, origin, |idx| {
            paragraph.grapheme_position(0, idx).map(|point| point.x)
        }) else {
            continue;
        };
        tracing::trace!(
            target: TARGET_OCR_TEXT,
            item_index = *index,
            scale,
            origin_x = origin.x,
            origin_y = origin.y,
            item_x = item.bounds.x,
            item_y = item.bounds.y,
            item_width = item.bounds.width,
            item_height = item.bounds.height,
            start_char = highlight.start_char,
            end_char = highlight.end_char,
            start_frac = highlight.start_frac,
            end_frac = highlight.end_frac,
            paragraph_start_x = highlight.paragraph_start_x,
            paragraph_end_x = highlight.paragraph_end_x,
            fallback_start_x = highlight.fallback_start_x,
            fallback_end_x = highlight.fallback_end_x,
            highlight_x = highlight.bounds.x,
            highlight_y = highlight.bounds.y,
            highlight_width = highlight.bounds.width,
            highlight_height = highlight.bounds.height,
            "ocr selection highlight"
        );
        renderer.fill_quad(
            renderer::Quad {
                bounds: highlight.bounds,
                ..renderer::Quad::default()
            },
            color,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::super::ocr_text::{OcrItemId, OcrTextItem};
    use super::*;

    fn rect(x: f32, y: f32, width: f32, height: f32) -> ImageRect {
        ImageRect {
            x,
            y,
            width,
            height,
        }
    }

    fn item(text: &str, bounds: ImageRect) -> OcrTextItem {
        OcrTextItem {
            id: OcrItemId(0),
            text: text.to_string(),
            confidence: 0.95,
            bounds,
            quad: [
                ImagePoint {
                    x: bounds.x,
                    y: bounds.y,
                },
                ImagePoint {
                    x: bounds.x + bounds.width,
                    y: bounds.y,
                },
                ImagePoint {
                    x: bounds.x + bounds.width,
                    y: bounds.y + bounds.height,
                },
                ImagePoint {
                    x: bounds.x,
                    y: bounds.y + bounds.height,
                },
            ],
        }
    }

    fn assert_close(actual: f32, expected: f32) {
        assert!(
            (actual - expected).abs() <= 0.001,
            "expected {expected}, got {actual}"
        );
    }

    #[test]
    fn cursor_mapping_uses_widget_local_coordinates() {
        let bounds = Rectangle {
            x: 300.0,
            y: 200.0,
            width: 400.0,
            height: 300.0,
        };

        let point = image_point_from_cursor(
            mouse::Cursor::Available(Point::new(340.0, 230.0)),
            bounds,
            2.0,
        )
        .unwrap();

        assert_eq!(point, ImagePoint { x: 20.0, y: 15.0 });
    }

    #[test]
    fn cursor_mapping_rejects_points_outside_layer_bounds() {
        let bounds = Rectangle {
            x: 300.0,
            y: 200.0,
            width: 400.0,
            height: 300.0,
        };

        assert_eq!(
            image_point_from_cursor(
                mouse::Cursor::Available(Point::new(299.0, 230.0)),
                bounds,
                2.0,
            ),
            None
        );
    }

    #[test]
    fn selection_highlight_uses_grapheme_positions_not_average_width() {
        let item = item("wide", rect(20.0, 30.0, 100.0, 10.0));
        let origin = Point::new(7.0, 11.0);
        let scale = 2.0;
        let grapheme_x = [0.0, 5.0, 17.0, 29.0, 40.0];

        let highlight = selection_highlight(
            0,
            &item,
            TextCursor::new(0, 1),
            TextCursor::new(0, 3),
            scale,
            origin,
            |idx| grapheme_x.get(idx).copied(),
        )
        .unwrap();

        assert_close(highlight.bounds.x, 52.0);
        assert_close(highlight.bounds.y, 71.0);
        assert_close(highlight.bounds.width, 24.0);
        assert_close(highlight.bounds.height, 20.0);
        assert_close(highlight.fallback_start_x, 50.0);
        assert_close(highlight.fallback_end_x, 150.0);
    }

    #[test]
    fn selection_highlight_falls_back_to_axis_aligned_average_width() {
        let item = item("wide", rect(20.0, 30.0, 100.0, 10.0));
        let origin = Point::new(7.0, 11.0);
        let scale = 2.0;

        let highlight = selection_highlight(
            0,
            &item,
            TextCursor::new(0, 1),
            TextCursor::new(0, 3),
            scale,
            origin,
            |_| None,
        )
        .unwrap();

        assert_close(highlight.bounds.x, 97.0);
        assert_close(highlight.bounds.y, 71.0);
        assert_close(highlight.bounds.width, 100.0);
        assert_close(highlight.bounds.height, 20.0);
    }
}
