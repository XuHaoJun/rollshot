use iced::advanced::layout::{self, Layout};
use iced::advanced::renderer;
use iced::advanced::text::{self, Paragraph};
use iced::advanced::widget::{self, tree, Widget};
use iced::advanced::{mouse, Clipboard, Shell};
use iced::{alignment, Color, Element, Event, Length, Point, Rectangle, Size};
use rollshot_image_document::{ImagePoint, ImageRect};

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
                state.paragraphs.push((index, Renderer::Paragraph::with_text(text::Text {
                    content: item.text.as_str(),
                    bounds: Size::new(item.bounds.width * self.scale, item.bounds.height * self.scale),
                    size: iced::Pixels((item.bounds.height * self.scale).max(8.0)),
                    line_height: text::LineHeight::Relative(1.0),
                    font: renderer.default_font(),
                    align_x: text::Alignment::Left,
                    align_y: alignment::Vertical::Center,
                    shaping: text::Shaping::Advanced,
                    wrapping: text::Wrapping::None,
                })));
            }
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
                let Some(local) = cursor.position_over(layout.bounds()) else {
                    return;
                };
                let point = ImagePoint {
                    x: local.x / self.scale,
                    y: local.y / self.scale,
                };
                if let Some(hit) = hit_test(document, &state.paragraphs, self.scale, point) {
                    state.dragging = true;
                    shell.publish(Message::OcrSelectionStarted(hit).into());
                    shell.capture_event();
                }
            }
            Event::Mouse(mouse::Event::CursorMoved { .. }) => {
                if !state.dragging {
                    return;
                }
                let Some(local) = cursor.position_over(layout.bounds()) else {
                    shell.capture_event();
                    return;
                };
                let point = ImagePoint {
                    x: local.x / self.scale,
                    y: local.y / self.scale,
                };
                if let Some(hit) = hit_test(document, &state.paragraphs, self.scale, point) {
                    shell.publish(Message::OcrSelectionChanged(hit).into());
                }
                shell.capture_event();
            }
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                if !state.dragging {
                    return;
                }
                state.dragging = false;
                let Some(local) = cursor.position_over(layout.bounds()) else {
                    shell.capture_event();
                    return;
                };
                let point = ImagePoint {
                    x: local.x / self.scale,
                    y: local.y / self.scale,
                };
                if let Some(hit) = hit_test(document, &state.paragraphs, self.scale, point) {
                    shell.publish(Message::OcrSelectionFinished(hit).into());
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
            state.paragraphs.iter().map(|(index, _)| *index),
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
        let Some(local) = cursor.position_over(layout.bounds()) else {
            return mouse::Interaction::default();
        };
        let state = tree.state.downcast_ref::<State<Renderer>>();
        let point = ImagePoint {
            x: local.x / self.scale,
            y: local.y / self.scale,
        };
        if hit_test(document, &state.paragraphs, self.scale, point).is_some() {
            mouse::Interaction::Text
        } else {
            mouse::Interaction::default()
        }
    }
}

impl<'a, MessageT, Theme, Renderer> From<OcrTextLayer<'a>> for Element<'a, MessageT, Theme, Renderer>
where
    MessageT: From<Message> + Clone + 'a,
    Renderer: renderer::Renderer + text::Renderer<Font = iced::Font> + 'static + 'a,
    Theme: 'a,
{
    fn from(layer: OcrTextLayer<'a>) -> Self {
        Element::new(layer)
    }
}

fn hit_test<ParagraphT>(
    document: &OcrTextDocument,
    paragraphs: &[(usize, ParagraphT)],
    scale: f32,
    point: ImagePoint,
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
            .map(|hit| hit.cursor())
            .unwrap_or_else(|| super::ocr_text::character_index_for_axis_aligned_item(item, point));
        return Some(TextCursor::new(*index, char_index));
    }
    None
}

fn draw_selection<Renderer>(
    renderer: &mut Renderer,
    document: &OcrTextDocument,
    selection: Option<OcrSelection>,
    scale: f32,
    origin: Point,
    visible_indices: impl IntoIterator<Item = usize>,
) where
    Renderer: renderer::Renderer,
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
    for index in visible_indices {
        let item = &document.visible_items()[index];
        if index < start.item_index || index > end.item_index {
            continue;
        }
        let chars = item.text.chars().count().max(1);
        let start_frac = if index == start.item_index {
            start.char_index as f32 / chars as f32
        } else {
            0.0
        };
        let end_frac = if index == end.item_index {
            end.char_index as f32 / chars as f32
        } else {
            1.0
        };
        if end_frac <= start_frac {
            continue;
        }
        let x = item.bounds.x + item.bounds.width * start_frac;
        let w = item.bounds.width * (end_frac - start_frac);
        renderer.fill_quad(
            renderer::Quad {
                bounds: Rectangle {
                    x: origin.x + x * scale,
                    y: origin.y + item.bounds.y * scale,
                    width: w * scale,
                    height: item.bounds.height * scale,
                },
                ..renderer::Quad::default()
            },
            color,
        );
    }
}
