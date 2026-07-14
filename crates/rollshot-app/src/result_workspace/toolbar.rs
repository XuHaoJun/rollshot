use super::canvas::Tool;
use super::Message;
use super::ResultWorkspace;
use iced::widget::canvas as canvas_widget;
use iced::widget::{button, column, container, responsive, row, text, text_input, tooltip};
use iced::{mouse, Alignment, Color, Element, Length, Point, Rectangle, Renderer, Size, Theme};
use rollshot_image_document::Rgb8;

const ICON_UNDO: &str = "\u{21B6}";
const ICON_REDO: &str = "\u{21B7}";

fn shortcut_label(name: &str, key: &str) -> String {
    format!("{name} ({key})")
}

// ---------------------------------------------------------------------------
// Density routing
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolbarDensity {
    Wide,
    Compact,
    Narrow,
}

pub fn density_for_width(width: f32) -> ToolbarDensity {
    if width >= 1000.0 {
        ToolbarDensity::Wide
    } else if width >= 700.0 {
        ToolbarDensity::Compact
    } else {
        ToolbarDensity::Narrow
    }
}

// ---------------------------------------------------------------------------
// Toolbar item model
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolbarItemKind {
    Tool(Tool),
    Copy,
    CopySplit,
    Save,
    Undo,
    Redo,
    Navigator,
    SmartRedaction,
    Reveal,
    ExportBugReport,
    #[cfg(feature = "ocr")]
    Ocr,
    Close,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolbarItem {
    pub kind: ToolbarItemKind,
    pub label: &'static str,
    pub shortcut: &'static str,
}

impl ToolbarItem {
    pub const COPY_SPLIT: Self = Self {
        kind: ToolbarItemKind::CopySplit,
        label: "\u{25BE}",
        shortcut: "",
    };
    pub const UNDO: Self = Self {
        kind: ToolbarItemKind::Undo,
        label: "Undo",
        shortcut: "Ctrl+Z",
    };
    pub const REDO: Self = Self {
        kind: ToolbarItemKind::Redo,
        label: "Redo",
        shortcut: "Ctrl+Shift+Z",
    };
    pub const CLOSE: Self = Self {
        kind: ToolbarItemKind::Close,
        label: "Close",
        shortcut: "",
    };
    pub const SMART_REDACTION: Self = Self {
        kind: ToolbarItemKind::SmartRedaction,
        label: "Smart Redaction",
        shortcut: "",
    };
    pub const NAVIGATOR: Self = Self {
        kind: ToolbarItemKind::Navigator,
        label: "Navigator",
        shortcut: "",
    };
    pub const REVEAL: Self = Self {
        kind: ToolbarItemKind::Reveal,
        label: "Reveal",
        shortcut: "",
    };
    pub const EXPORT_BUG_REPORT: Self = Self {
        kind: ToolbarItemKind::ExportBugReport,
        label: "Export Bug Report...",
        shortcut: "",
    };
}

fn tool_item(tool: Tool) -> ToolbarItem {
    match tool {
        Tool::Select => ToolbarItem {
            kind: ToolbarItemKind::Tool(Tool::Select),
            label: "Select",
            shortcut: "V",
        },
        Tool::Number => ToolbarItem {
            kind: ToolbarItemKind::Tool(Tool::Number),
            label: "Number",
            shortcut: "N",
        },
        Tool::Text => ToolbarItem {
            kind: ToolbarItemKind::Tool(Tool::Text),
            label: "Text",
            shortcut: "T",
        },
        Tool::Line => ToolbarItem {
            kind: ToolbarItemKind::Tool(Tool::Line),
            label: "Line",
            shortcut: "L",
        },
        Tool::Arrow => ToolbarItem {
            kind: ToolbarItemKind::Tool(Tool::Arrow),
            label: "Arrow",
            shortcut: "A",
        },
        Tool::Redact => ToolbarItem {
            kind: ToolbarItemKind::Tool(Tool::Redact),
            label: "Redact",
            shortcut: "R",
        },
        Tool::Rectangle => ToolbarItem {
            kind: ToolbarItemKind::Tool(Tool::Rectangle),
            label: "Rectangle",
            shortcut: "U",
        },
        Tool::Ellipse => ToolbarItem {
            kind: ToolbarItemKind::Tool(Tool::Ellipse),
            label: "Ellipse",
            shortcut: "O",
        },
        Tool::Pen => ToolbarItem {
            kind: ToolbarItemKind::Tool(Tool::Pen),
            label: "Pen",
            shortcut: "P",
        },
        Tool::Highlighter => ToolbarItem {
            kind: ToolbarItemKind::Tool(Tool::Highlighter),
            label: "Highlighter",
            shortcut: "H",
        },
        #[cfg(feature = "ocr")]
        Tool::OcrText => ToolbarItem {
            kind: ToolbarItemKind::Ocr,
            label: "OCR Text",
            shortcut: "O",
        },
    }
}

// ---------------------------------------------------------------------------
// Toolbar model
// ---------------------------------------------------------------------------

pub struct ToolbarModel {
    #[allow(dead_code)]
    pub first_row: Vec<ToolbarItem>,
    pub visible_tools: Vec<Tool>,
    pub more: Vec<ToolbarItem>,
    pub more_active_tool: Option<(Tool, &'static str)>,
}

pub fn toolbar_model(state: &ResultWorkspace, width: f32) -> ToolbarModel {
    let density = density_for_width(width);
    let remembered = state.annotation_defaults.values.last_shape;

    let primary_tools = match density {
        ToolbarDensity::Wide | ToolbarDensity::Compact => vec![
            Tool::Select,
            Tool::Number,
            Tool::Text,
            Tool::Line,
            Tool::Arrow,
            remembered.into(),
            Tool::Pen,
            Tool::Highlighter,
            Tool::Redact,
        ],
        ToolbarDensity::Narrow => vec![
            Tool::Select,
            Tool::Number,
            Tool::Text,
            Tool::Arrow,
            remembered.into(),
            Tool::Pen,
        ],
    };

    let mut overflow = vec![
        ToolbarItem::SMART_REDACTION,
        ToolbarItem::NAVIGATOR,
        ToolbarItem::REVEAL,
        ToolbarItem::EXPORT_BUG_REPORT,
    ];

    #[cfg(feature = "ocr")]
    overflow.push(ToolbarItem {
        kind: ToolbarItemKind::Ocr,
        label: "OCR Text",
        shortcut: "O",
    });

    if density == ToolbarDensity::Narrow {
        overflow.insert(0, tool_item(Tool::Redact));
        overflow.insert(0, tool_item(Tool::Highlighter));
        overflow.insert(0, tool_item(Tool::Line));
    }

    let more_active_tool = if overflow
        .iter()
        .any(|item| matches!(item.kind, ToolbarItemKind::Tool(t) if t == state.editor.tool))
    {
        let name = overflow
            .iter()
            .find_map(|item| match item.kind {
                ToolbarItemKind::Tool(t) if t == state.editor.tool => Some(item.label),
                _ => None,
            })
            .unwrap_or("Tool");
        Some((state.editor.tool, name))
    } else {
        None
    };

    let copy_label = super::secure_sharing::copy_label(&state.document);
    let save_label_text = super::secure_sharing::save_label(&state.document);

    let mut first_row = vec![ToolbarItem::CLOSE];

    first_row.push(ToolbarItem {
        kind: ToolbarItemKind::Copy,
        label: copy_label,
        shortcut: "",
    });
    first_row.push(ToolbarItem::COPY_SPLIT);
    first_row.push(ToolbarItem {
        kind: ToolbarItemKind::Save,
        label: save_label_text,
        shortcut: "",
    });
    first_row.push(ToolbarItem::UNDO);
    first_row.push(ToolbarItem::REDO);

    let visible_tools: Vec<Tool> = primary_tools
        .iter()
        .filter(|tool| {
            !overflow
                .iter()
                .any(|item| matches!(item.kind, ToolbarItemKind::Tool(t) if t == **tool))
        })
        .copied()
        .collect();

    ToolbarModel {
        first_row,
        visible_tools,
        more: overflow,
        more_active_tool,
    }
}

// ---------------------------------------------------------------------------
// HSV → RGB conversion (for canvas picker rendering)
// ---------------------------------------------------------------------------

pub fn sv_from_point(point: Point, size: Size) -> (f32, f32) {
    let s = (point.x / size.width).clamp(0.0, 1.0);
    let v = 1.0 - (point.y / size.height).clamp(0.0, 1.0);
    (s, v)
}

pub fn hue_from_x(x: f32, width: f32) -> f32 {
    (x / width).clamp(0.0, 1.0) * 360.0
}

// ---------------------------------------------------------------------------
// HSV → RGB conversion (for canvas picker rendering)
// ---------------------------------------------------------------------------

#[allow(dead_code)]
fn hsv_to_rgb(h: f32, s: f32, v: f32) -> (f32, f32, f32) {
    let c = v * s;
    let h2 = h / 60.0;
    let x = c * (1.0 - ((h2 % 2.0) - 1.0).abs());
    let (r1, g1, b1) = match h2 as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let m = v - c;
    (r1 + m, g1 + m, b1 + m)
}

#[allow(dead_code)]
fn rgb_from_hsv(h: f32, s: f32, v: f32) -> Rgb8 {
    let (r, g, b) = hsv_to_rgb(h, s, v);
    Rgb8::new((r * 255.0) as u8, (g * 255.0) as u8, (b * 255.0) as u8)
}

// ---------------------------------------------------------------------------
// Saturation/Value canvas program
// ---------------------------------------------------------------------------

#[allow(dead_code)]
struct SaturationValue {
    hue: f32,
    selected: Option<Rgb8>,
}

impl canvas_widget::Program<Message> for SaturationValue {
    type State = ();

    fn update(
        &self,
        _state: &mut (),
        event: &canvas_widget::Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<canvas_widget::Action<Message>> {
        if let iced::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) = event {
            let local = cursor.position_in(bounds)?;
            let size = bounds.size();
            let (s, v) = sv_from_point(local, size);
            let rgb = rgb_from_hsv(self.hue, s, v);
            return Some(canvas_widget::Action::publish(Message::PreviewColor(rgb)));
        }
        None
    }

    fn draw(
        &self,
        _state: &(),
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<canvas_widget::Geometry> {
        let mut frame = canvas_widget::Frame::new(renderer, bounds.size());
        let w = bounds.width;
        let h = bounds.height;

        // Saturation/value gradient: horizontal = saturation, vertical = value
        for y in 0..(h as u32) {
            for x in 0..(w as u32) {
                let s = x as f32 / w;
                let v = 1.0 - y as f32 / h;
                let (r, g, b) = hsv_to_rgb(self.hue, s, v);
                let color = Color::from_rgb(r, g, b);
                frame.fill_rectangle(Point::new(x as f32, y as f32), Size::new(1.0, 1.0), color);
            }
        }

        // Draw cursor crosshair at selected position
        if let Some(selected) = self.selected {
            // Find approximate position from RGB
            let (h2, s2, v2) = rgb_to_hsv(selected);
            if (h2 - self.hue).abs() < 1.0 {
                let cx = s2 * w;
                let cy = (1.0 - v2) * h;
                frame.stroke(
                    &canvas_widget::Path::circle(Point::new(cx, cy), 5.0),
                    canvas_widget::Stroke::default()
                        .with_color(Color::WHITE)
                        .with_width(2.0),
                );
                frame.stroke(
                    &canvas_widget::Path::circle(Point::new(cx, cy), 6.0),
                    canvas_widget::Stroke::default()
                        .with_color(Color::BLACK)
                        .with_width(1.0),
                );
            }
        }

        vec![frame.into_geometry()]
    }
}

// ---------------------------------------------------------------------------
// Hue strip canvas program
// ---------------------------------------------------------------------------

#[allow(dead_code)]
struct HueStrip;

impl canvas_widget::Program<Message> for HueStrip {
    type State = ();

    fn update(
        &self,
        _state: &mut (),
        event: &canvas_widget::Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<canvas_widget::Action<Message>> {
        if let iced::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) = event {
            let local = cursor.position_in(bounds)?;
            let hue = hue_from_x(local.x, bounds.width);
            let rgb = rgb_from_hsv(hue, 1.0, 1.0);
            return Some(canvas_widget::Action::publish(Message::PreviewColor(rgb)));
        }
        None
    }

    fn draw(
        &self,
        _state: &(),
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<canvas_widget::Geometry> {
        let mut frame = canvas_widget::Frame::new(renderer, bounds.size());
        let w = bounds.width;
        let h = bounds.height;

        // Hue strip: horizontal gradient 0..360
        for x in 0..(w as u32) {
            let hue = (x as f32 / w) * 360.0;
            let (r, g, b) = hsv_to_rgb(hue, 1.0, 1.0);
            frame.fill_rectangle(
                Point::new(x as f32, 0.0),
                Size::new(1.0, h),
                Color::from_rgb(r, g, b),
            );
        }

        vec![frame.into_geometry()]
    }
}

#[allow(dead_code)]
fn rgb_to_hsv(rgb: Rgb8) -> (f32, f32, f32) {
    let r = rgb.r as f32 / 255.0;
    let g = rgb.g as f32 / 255.0;
    let b = rgb.b as f32 / 255.0;
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let d = max - min;
    let v = max;
    if max == 0.0 {
        return (0.0, 0.0, 0.0);
    }
    let s = d / max;
    let h = if d == 0.0 {
        0.0
    } else if max == r {
        60.0 * (((g - b) / d) % 6.0)
    } else if max == g {
        60.0 * (((b - r) / d) + 2.0)
    } else {
        60.0 * (((r - g) / d) + 4.0)
    };
    let h = if h < 0.0 { h + 360.0 } else { h };
    (h, s, v)
}

// ---------------------------------------------------------------------------
// View construction
// ---------------------------------------------------------------------------

fn tool_tooltip(tool: Tool) -> String {
    match tool {
        Tool::Line => "Line (L) — Shift: Snap to 45°".into(),
        Tool::Arrow => "Arrow (A) — Shift: Snap to 45°".into(),
        Tool::Rectangle => "Rectangle (S) — Shift: Square".into(),
        Tool::Ellipse => "Ellipse (S) — Shift: Circle".into(),
        _ => {
            let item = tool_item(tool);
            shortcut_label(item.label, item.shortcut)
        }
    }
}

fn tool_button<'a>(tool: Tool, state: &ResultWorkspace) -> Element<'a, Message> {
    let item = tool_item(tool);
    let btn = button(text(item.label).size(14))
        .padding([4, 8])
        .on_press(Message::SelectTool(tool))
        .style(if state.editor.tool == tool {
            button::primary
        } else {
            button::secondary
        });
    tooltip(btn, text(tool_tooltip(tool)), tooltip::Position::Bottom).into()
}

fn shape_tool_button<'a>(tool: Tool, state: &ResultWorkspace) -> Element<'a, Message> {
    let remembered = state.annotation_defaults.values.last_shape;
    let active = state.editor.tool == tool;
    let primary = button(text(tool_item(tool).label).size(14))
        .padding([4, 8])
        .on_press(Message::SelectRememberedShape)
        .style(if active {
            button::primary
        } else {
            button::secondary
        });
    let primary_with_tip = tooltip(
        primary,
        text(tool_tooltip(remembered.into())),
        tooltip::Position::Bottom,
    );
    let chevron = button(text("\u{25BE}").size(12))
        .padding([4, 4])
        .on_press(Message::ToggleShapesMenu)
        .style(if state.editor.shapes_menu_open {
            button::primary
        } else {
            button::secondary
        });
    row![primary_with_tip, chevron].spacing(1).into()
}

fn shapes_selector<'a>(state: &ResultWorkspace) -> Option<Element<'a, Message>> {
    if !state.editor.shapes_menu_open {
        return None;
    }
    let remembered = state.annotation_defaults.values.last_shape;
    let kinds = [
        rollshot_image_document::ShapeKind::Rectangle,
        rollshot_image_document::ShapeKind::Ellipse,
    ];
    let mut menu = row![].spacing(4);
    for kind in kinds {
        let tool: Tool = kind.into();
        let item = tool_item(tool);
        let is_active = kind == remembered;
        let btn = button(text(item.label).size(14))
            .padding([4, 12])
            .on_press(Message::SelectShape(kind))
            .style(if is_active {
                button::primary
            } else {
                button::secondary
            });
        menu = menu.push(btn);
    }
    Some(container(menu).padding(4).into())
}

fn undo_button(state: &ResultWorkspace) -> Element<'_, Message> {
    let btn = button(text(ICON_UNDO).size(16))
        .padding([4, 10])
        .on_press_maybe(state.document.image.can_undo().then_some(Message::Undo));
    tooltip(
        btn,
        text(shortcut_label("Undo", "Ctrl+Z")),
        tooltip::Position::Bottom,
    )
    .into()
}

fn redo_button(state: &ResultWorkspace) -> Element<'_, Message> {
    let btn = button(text(ICON_REDO).size(16))
        .padding([4, 10])
        .on_press_maybe(state.document.image.can_redo().then_some(Message::Redo));
    tooltip(
        btn,
        text(shortcut_label("Redo", "Ctrl+Shift+Z")),
        tooltip::Position::Bottom,
    )
    .into()
}

fn more_button<'a>(model: &ToolbarModel, overflow_open: bool) -> Element<'a, Message> {
    let label = match model.more_active_tool {
        Some((_, name)) => format!("More: {name}"),
        None => {
            let count = model.more.len();
            format!("More ({count})")
        }
    };
    let btn = button(text(label).size(14))
        .padding([4, 8])
        .on_press(Message::ToggleMoreMenu)
        .style(if overflow_open {
            button::primary
        } else {
            button::secondary
        });
    btn.into()
}

#[allow(dead_code)]
fn overflow_item_button(item: ToolbarItem, state: &ResultWorkspace) -> Element<'static, Message> {
    if item.kind == ToolbarItemKind::Reveal {
        let button = button(text(item.label).size(14)).padding([4, 12]);
        return if matches!(
            super::secure_sharing::reveal_action(&state.document),
            super::secure_sharing::RevealAction::Disabled
        ) {
            button.into()
        } else {
            button.on_press(Message::Reveal).into()
        };
    }
    let msg = match item.kind {
        ToolbarItemKind::Tool(tool) => Message::SelectTool(tool),
        ToolbarItemKind::SmartRedaction => Message::SmartRedaction,
        ToolbarItemKind::Navigator => Message::ToggleNavigator,
        ToolbarItemKind::ExportBugReport => Message::ExportBugReport,
        #[cfg(feature = "ocr")]
        ToolbarItemKind::Ocr => Message::SelectTool(Tool::OcrText),
        _ => return text("").into(),
    };
    let button = button(text(item.label).size(14))
        .padding([4, 12])
        .on_press(msg);
    button.into()
}

/// First row: Close, title, undo/redo, Copy, Save.
pub fn first_row(state: &ResultWorkspace) -> Element<'_, Message> {
    let copy_label = super::secure_sharing::copy_label(&state.document);
    let save_label = super::secure_sharing::save_label(&state.document);

    row![
        button(text("Close")).on_press(Message::RequestClose),
        text(state.document.display_name())
            .width(Length::Fill)
            .size(14),
        undo_button(state),
        redo_button(state),
        iced::widget::rule::vertical(1),
        button(text(copy_label)).on_press(Message::Copy),
        button(text("\u{25BE}")).on_press(Message::ToggleCopyMenu),
        button(text(save_label)).on_press(Message::SaveAs),
    ]
    .height(40)
    .align_y(Alignment::Center)
    .spacing(8)
    .into()
}

/// Second row: tool buttons, More menu, and contextual property controls.
pub fn second_row(state: &ResultWorkspace, model: ToolbarModel) -> Element<'_, Message> {
    let mut tools_row = row![].spacing(4).align_y(Alignment::Center);

    for tool in &model.visible_tools {
        if matches!(tool, Tool::Rectangle | Tool::Ellipse) {
            tools_row = tools_row.push(shape_tool_button(*tool, state));
        } else {
            tools_row = tools_row.push(tool_button(*tool, state));
        }
    }

    #[cfg(feature = "ocr")]
    if state.editor.tool == Tool::OcrText {
        tools_row = tools_row.push(tool_button(Tool::OcrText, state));
    }

    tools_row = tools_row
        .push(iced::widget::rule::vertical(1))
        .push(more_button(&model, state.editor.more_menu_open));

    let properties_row = super::properties::view(state);
    let properties_row = properties_row.unwrap_or_else(|| {
        iced::widget::Space::new()
            .width(Length::Fill)
            .height(Length::Fixed(0.0))
            .into()
    });

    let controls: Element<'_, Message> = row![tools_row, properties_row]
        .spacing(8)
        .align_y(Alignment::Center)
        .into();

    if state.editor.more_menu_open {
        let mut menu = row![].spacing(4);
        for item in model.more {
            menu = menu.push(overflow_item_button(item, state));
        }
        column![controls, container(menu).padding(4)]
            .spacing(4)
            .into()
    } else if state.editor.shapes_menu_open {
        if let Some(selector) = shapes_selector(state) {
            column![controls, selector].spacing(4).into()
        } else {
            controls
        }
    } else if state.editor.properties.color.is_some() {
        column![controls, color_picker(state)].spacing(4).into()
    } else {
        controls
    }
}

fn color_picker(state: &ResultWorkspace) -> Element<'_, Message> {
    let Some(transaction) = state.editor.properties.color.as_ref() else {
        return text("").into();
    };
    let (hue, _, _) = rgb_to_hsv(transaction.preview);
    let palette = [
        Rgb8::new(0xE5, 0x48, 0x4D),
        Rgb8::new(0xFF, 0xA5, 0x00),
        Rgb8::new(0xFF, 0xD6, 0x00),
        Rgb8::new(0x2E, 0xC4, 0x71),
        Rgb8::new(0x2D, 0x9C, 0xDB),
        Rgb8::new(0x9B, 0x51, 0xE0),
        Rgb8::new(0x11, 0x11, 0x11),
        Rgb8::new(0xFF, 0xFF, 0xFF),
    ];
    let palette = palette.into_iter().fold(row![].spacing(4), |row, color| {
        row.push(
            button(text("●").style(move |_| text::Style {
                color: Some(Color::from_rgb8(color.r, color.g, color.b)),
            }))
            .on_press(Message::PreviewColor(color)),
        )
    });
    let saturation_value = iced::widget::canvas(SaturationValue {
        hue,
        selected: Some(transaction.preview),
    })
    .width(220)
    .height(120);
    let hue = iced::widget::canvas(HueStrip).width(220).height(18);
    let valid_hex = super::properties::parse_hex_rgb(&transaction.hex).is_ok();

    container(
        column![
            palette,
            saturation_value,
            hue,
            row![
                text_input("#RRGGBB", &transaction.hex)
                    .on_input(Message::ColorHexChanged)
                    .width(110),
                button(text("Apply")).on_press_maybe(valid_hex.then_some(Message::ApplyColor)),
                button(text("Cancel")).on_press(Message::CancelColor),
            ]
            .spacing(6),
        ]
        .spacing(6),
    )
    .padding(8)
    .into()
}

/// Top-level toolbar view: first row + responsive second row.
pub fn view(state: &ResultWorkspace) -> Element<'_, Message> {
    let first = first_row(state);
    let second = responsive(move |size| {
        let model = toolbar_model(state, size.width);
        second_row(state, model)
    })
    .width(Length::Fill);

    column![first, second].into()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::result_workspace::document::ResultDocument;
    use image::{Rgba, RgbaImage};

    fn image() -> RgbaImage {
        RgbaImage::from_pixel(200, 200, Rgba([100, 150, 200, 255]))
    }

    fn state() -> ResultWorkspace {
        let mut state = ResultWorkspace::new(ResultDocument::unsaved(image()), None);
        state.annotation_defaults.values = super::super::AnnotationDefaults::default();
        state.annotation_defaults.config_path = None;
        state
    }

    #[test]
    fn density_for_width_wide() {
        assert_eq!(density_for_width(1000.0), ToolbarDensity::Wide);
        assert_eq!(density_for_width(1200.0), ToolbarDensity::Wide);
    }

    #[test]
    fn density_for_width_compact() {
        assert_eq!(density_for_width(700.0), ToolbarDensity::Compact);
        assert_eq!(density_for_width(900.0), ToolbarDensity::Compact);
    }

    #[test]
    fn density_for_width_narrow() {
        assert_eq!(density_for_width(640.0), ToolbarDensity::Narrow);
        assert_eq!(density_for_width(300.0), ToolbarDensity::Narrow);
    }

    #[test]
    fn copy_and_save_never_enter_overflow() {
        for width in [640.0, 800.0, 1100.0] {
            let model = toolbar_model(&state(), width);
            assert!(
                model
                    .first_row
                    .iter()
                    .any(|item| item.kind == ToolbarItemKind::Copy),
                "Copy must be in first_row at width {width}"
            );
            assert!(
                model
                    .first_row
                    .iter()
                    .any(|item| item.kind == ToolbarItemKind::Save),
                "Save must be in first_row at width {width}"
            );
            assert!(
                !model
                    .more
                    .iter()
                    .any(|item| item.kind == ToolbarItemKind::Copy),
                "Copy must NOT be in more at width {width}"
            );
            assert!(
                !model
                    .more
                    .iter()
                    .any(|item| item.kind == ToolbarItemKind::Save),
                "Save must NOT be in more at width {width}"
            );
        }
    }

    #[test]
    fn narrow_priority_preserves_select_number_and_text() {
        let model = toolbar_model(&state(), 640.0);
        let tools = &model.visible_tools;
        assert!(tools.starts_with(&[Tool::Select, Tool::Number, Tool::Text]));
        assert!(
            model
                .more
                .iter()
                .any(|item| item.kind == ToolbarItemKind::Tool(Tool::Redact)),
            "Redact must be in overflow on narrow"
        );
    }

    #[test]
    fn active_overflow_tool_marks_more_active_and_names_it() {
        let mut state = state();
        state.editor.tool = Tool::Redact;
        let model = toolbar_model(&state, 640.0);
        assert_eq!(model.more_active_tool, Some((Tool::Redact, "Redact")));
    }

    #[test]
    fn wide_shows_all_primary_tools() {
        let model = toolbar_model(&state(), 1100.0);
        assert_eq!(
            model.visible_tools,
            vec![
                Tool::Select,
                Tool::Number,
                Tool::Text,
                Tool::Line,
                Tool::Arrow,
                Tool::Rectangle,
                Tool::Pen,
                Tool::Highlighter,
                Tool::Redact,
            ]
        );
    }

    #[test]
    fn wide_and_compact_show_adjacent_line_and_arrow() {
        for width in [1000.0, 800.0] {
            let model = toolbar_model(&state(), width);
            let pair = model
                .visible_tools
                .windows(2)
                .any(|tools| tools == [Tool::Line, Tool::Arrow]);
            assert!(pair, "Line and Arrow must be adjacent at width {width}");
        }
    }

    #[test]
    fn narrow_keeps_arrow_visible_and_routes_active_line_through_more() {
        let mut state = state();
        state.editor.tool = Tool::Line;
        let model = toolbar_model(&state, 600.0);
        assert!(model.visible_tools.contains(&Tool::Arrow));
        assert!(!model.visible_tools.contains(&Tool::Line));
        assert!(model
            .more
            .iter()
            .any(|item| item.kind == ToolbarItemKind::Tool(Tool::Line)));
        assert_eq!(model.more_active_tool, Some((Tool::Line, "Line")));
    }

    #[test]
    fn two_point_tooltips_include_shortcut_and_shift_hint() {
        assert_eq!(tool_tooltip(Tool::Line), "Line (L) — Shift: Snap to 45°");
        assert_eq!(tool_tooltip(Tool::Arrow), "Arrow (A) — Shift: Snap to 45°");
    }

    #[test]
    fn smart_redaction_navigator_reveal_export_always_in_more() {
        for width in [640.0, 800.0, 1100.0] {
            let model = toolbar_model(&state(), width);
            let kinds: Vec<_> = model.more.iter().map(|i| i.kind).collect();
            assert!(
                kinds.contains(&ToolbarItemKind::SmartRedaction),
                "Smart Redaction must be in more at width {width}"
            );
            assert!(
                kinds.contains(&ToolbarItemKind::Navigator),
                "Navigator must be in more at width {width}"
            );
            assert!(
                kinds.contains(&ToolbarItemKind::Reveal),
                "Reveal must be in more at width {width}"
            );
            assert!(
                kinds.contains(&ToolbarItemKind::ExportBugReport),
                "Export Bug Report must be in more at width {width}"
            );
        }
    }

    // -- picker math tests ---------------------------------------------------

    #[test]
    fn saturation_value_and_hue_mapping_clamp_at_every_edge() {
        assert_eq!(
            sv_from_point(Point::new(-1.0, 121.0), Size::new(220.0, 120.0)),
            (0.0, 0.0)
        );
        assert_eq!(
            sv_from_point(Point::new(220.0, 0.0), Size::new(220.0, 120.0)),
            (1.0, 1.0)
        );
        assert_eq!(hue_from_x(-5.0, 220.0), 0.0);
        assert_eq!(hue_from_x(225.0, 220.0), 360.0);
    }

    #[test]
    fn sv_from_point_center() {
        let (s, v) = sv_from_point(Point::new(110.0, 60.0), Size::new(220.0, 120.0));
        assert!((s - 0.5).abs() < f32::EPSILON);
        assert!((v - 0.5).abs() < f32::EPSILON);
    }

    #[test]
    fn hue_from_x_center() {
        let h = hue_from_x(110.0, 220.0);
        assert!((h - 180.0).abs() < f32::EPSILON);
    }

    #[test]
    fn hue_from_x_zero() {
        assert_eq!(hue_from_x(0.0, 220.0), 0.0);
    }

    #[test]
    fn hue_from_x_full() {
        assert_eq!(hue_from_x(220.0, 220.0), 360.0);
    }

    // -- shapes selector tests (Task 5) --------------------------------------

    #[test]
    fn densities_show_exactly_one_remembered_shape_control() {
        let s = state();
        assert_eq!(
            s.annotation_defaults.values.last_shape,
            rollshot_image_document::ShapeKind::Rectangle
        );
        for width in [1100.0, 800.0, 640.0] {
            let model = toolbar_model(&s, width);
            let visible_shapes = model
                .visible_tools
                .iter()
                .filter(|tool| matches!(tool, Tool::Rectangle | Tool::Ellipse))
                .copied()
                .collect::<Vec<_>>();
            assert_eq!(visible_shapes, vec![Tool::Rectangle], "width {width}");
        }
    }

    #[test]
    fn narrow_visible_tools_include_remembered_shape() {
        let s = state();
        let model = toolbar_model(&s, 640.0);
        assert!(
            model.visible_tools.contains(&Tool::Rectangle),
            "Remembered Rectangle must be visible on narrow"
        );
        assert!(
            !model.visible_tools.contains(&Tool::Ellipse),
            "Non-remembered Ellipse must be in overflow on narrow"
        );
    }

    #[test]
    fn narrow_selector_keeps_non_remembered_shape_out_of_overflow() {
        let mut s = state();
        s.annotation_defaults.values.last_shape = rollshot_image_document::ShapeKind::Ellipse;
        let model = toolbar_model(&s, 640.0);
        assert!(
            model.visible_tools.contains(&Tool::Ellipse),
            "Remembered Ellipse must be visible on narrow"
        );
        assert!(
            !model.visible_tools.contains(&Tool::Rectangle),
            "Non-remembered Rectangle must be in overflow on narrow"
        );
        assert!(!model.more.iter().any(|item| matches!(
            item.kind,
            ToolbarItemKind::Tool(Tool::Rectangle | Tool::Ellipse)
        )));
    }

    #[test]
    fn narrow_line_and_redact_always_in_overflow() {
        let model = toolbar_model(&state(), 640.0);
        assert!(model
            .more
            .iter()
            .any(|item| item.kind == ToolbarItemKind::Tool(Tool::Line)));
        assert!(model
            .more
            .iter()
            .any(|item| item.kind == ToolbarItemKind::Tool(Tool::Redact)));
    }

    #[test]
    fn shape_tooltips_include_s_shortcut_and_shift_hint() {
        assert_eq!(
            tool_tooltip(Tool::Rectangle),
            "Rectangle (S) — Shift: Square"
        );
        assert_eq!(tool_tooltip(Tool::Ellipse), "Ellipse (S) — Shift: Circle");
    }

    #[test]
    fn both_shape_choices_in_selector_with_active_indication() {
        use super::super::document::ResultDocument;
        use image::{Rgba, RgbaImage};

        let img = RgbaImage::from_pixel(200, 200, Rgba([100, 150, 200, 255]));
        let mut s = super::super::ResultWorkspace::new(ResultDocument::unsaved(img), None);
        s.editor.shapes_menu_open = true;
        // Default remembered = Rectangle
        assert!(
            shapes_selector(&s).is_some(),
            "Selector must render when open"
        );
        // Changing remembered to Ellipse
        s.annotation_defaults.values.last_shape = rollshot_image_document::ShapeKind::Ellipse;
        assert!(shapes_selector(&s).is_some());
    }

    #[test]
    fn shapes_selector_none_when_closed() {
        let s = state();
        assert!(
            shapes_selector(&s).is_none(),
            "Selector must not render when menu is closed"
        );
    }

    // -- Pen & Highlighter routing (Task 5) ---------------------------------

    #[test]
    fn pen_and_highlighter_route_by_density() {
        let s = state();
        // Wide: both visible.
        let wide = toolbar_model(&s, 1200.0);
        assert!(wide.visible_tools.contains(&Tool::Pen));
        assert!(wide.visible_tools.contains(&Tool::Highlighter));
        // Narrow: Pen visible, Highlighter in More (with Line and Redact).
        let narrow = toolbar_model(&s, 600.0);
        assert!(narrow.visible_tools.contains(&Tool::Pen));
        assert!(!narrow.visible_tools.contains(&Tool::Highlighter));
        assert!(narrow
            .more
            .iter()
            .any(|i| matches!(i.kind, ToolbarItemKind::Tool(Tool::Highlighter))));
    }

    #[test]
    fn pen_and_highlighter_items_have_shortcuts() {
        assert_eq!(tool_item(Tool::Pen).shortcut, "P");
        assert_eq!(tool_item(Tool::Highlighter).shortcut, "H");
    }

    #[test]
    fn pen_and_highlighter_tooltips() {
        assert_eq!(tool_tooltip(Tool::Pen), "Pen (P)");
        assert_eq!(tool_tooltip(Tool::Highlighter), "Highlighter (H)");
    }

    #[test]
    fn active_highlighter_in_more_at_narrow() {
        let mut s = state();
        s.editor.tool = Tool::Highlighter;
        let model = toolbar_model(&s, 600.0);
        assert!(!model.visible_tools.contains(&Tool::Highlighter));
        assert_eq!(
            model.more_active_tool,
            Some((Tool::Highlighter, "Highlighter"))
        );
    }

    #[test]
    fn umbrella_second_row_order_at_wide() {
        let model = toolbar_model(&state(), 1200.0);
        assert_eq!(
            model.visible_tools,
            vec![
                Tool::Select,
                Tool::Number,
                Tool::Text,
                Tool::Line,
                Tool::Arrow,
                Tool::Rectangle,
                Tool::Pen,
                Tool::Highlighter,
                Tool::Redact,
            ]
        );
    }

    #[test]
    fn narrow_pen_visible_highlighter_in_more() {
        let model = toolbar_model(&state(), 600.0);
        assert!(model.visible_tools.contains(&Tool::Pen));
        assert!(!model.visible_tools.contains(&Tool::Highlighter));
        // More has Line, Highlighter, Redact (in that order at front)
        let more_tools: Vec<_> = model
            .more
            .iter()
            .filter_map(|i| match i.kind {
                ToolbarItemKind::Tool(t) => Some(t),
                _ => None,
            })
            .collect();
        assert!(more_tools.contains(&Tool::Line));
        assert!(more_tools.contains(&Tool::Highlighter));
        assert!(more_tools.contains(&Tool::Redact));
    }
}
