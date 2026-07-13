use iced::Element;

use rollshot_image_document::{
    Annotation, AnnotationId, Rgb8, ShapeKind, StrokeStyle, TwoPointKind,
};

use super::canvas::Tool;
use super::Message;
use super::ResultWorkspace;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PropertyTarget {
    NumberTool,
    TextTool,
    TwoPointTool(TwoPointKind),
    ShapeTool(ShapeKind),
    Annotation(AnnotationId),
}

#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorProperty {
    NumberAccent,
    TextColor,
    TextBackground,
    StrokeColor,
    ShapeFill,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColorTransaction {
    pub target: PropertyTarget,
    pub property: ColorProperty,
    pub original: Rgb8,
    pub preview: Rgb8,
    pub hex: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct StrokeWidthTransaction {
    pub target: PropertyTarget,
    pub original: f32,
    pub preview: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ShapeStyleTransaction {
    pub id: AnnotationId,
    pub kind: ShapeKind,
    pub original_stroke: StrokeStyle,
    pub original_fill: Option<Rgb8>,
    pub preview_stroke: StrokeStyle,
    pub preview_fill: Option<Rgb8>,
    pub remembered_fill_color: Rgb8,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Popup {
    CopyMenu,
    MoreMenu,
    ColorPicker,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PropertyFocus {
    HexInput,
}

#[derive(Debug, Default)]
pub struct PropertyState {
    pub color: Option<ColorTransaction>,
    pub width: Option<StrokeWidthTransaction>,
    pub shape_style: Option<ShapeStyleTransaction>,
    pub next_number_input: String,
    #[allow(dead_code)]
    pub focus: Option<PropertyFocus>,
    pub popup: Option<Popup>,
}

/// Determine the active property target based on the current tool and selection.
pub fn property_target(state: &ResultWorkspace) -> Option<PropertyTarget> {
    match state.editor.tool {
        Tool::Number => Some(PropertyTarget::NumberTool),
        Tool::Text => Some(PropertyTarget::TextTool),
        Tool::Line => Some(PropertyTarget::TwoPointTool(TwoPointKind::Line)),
        Tool::Arrow => Some(PropertyTarget::TwoPointTool(TwoPointKind::Arrow)),
        Tool::Rectangle => Some(PropertyTarget::ShapeTool(ShapeKind::Rectangle)),
        Tool::Ellipse => Some(PropertyTarget::ShapeTool(ShapeKind::Ellipse)),
        Tool::Select => {
            state
                .editor
                .selection
                .and_then(|id| match state.document.image.annotation(id) {
                    Some(
                        Annotation::TwoPoint { .. }
                        | Annotation::NumberCallout { .. }
                        | Annotation::TextNote { .. }
                        | Annotation::Shape { .. },
                    ) => Some(PropertyTarget::Annotation(id)),
                    _ => None,
                })
        }
        Tool::Redact => None,
        #[cfg(feature = "ocr")]
        Tool::OcrText => None,
    }
}

/// Parse a hex RGB string like "#FF00AA" or "FF00AA" into an Rgb8.
pub fn parse_hex_rgb(input: &str) -> Result<Rgb8, &'static str> {
    let s = input.trim().trim_start_matches('#');
    if s.len() != 6 {
        return Err("hex color must be exactly 6 digits");
    }
    let r = u8::from_str_radix(&s[0..2], 16).map_err(|_| "invalid hex digit")?;
    let g = u8::from_str_radix(&s[2..4], 16).map_err(|_| "invalid hex digit")?;
    let b = u8::from_str_radix(&s[4..6], 16).map_err(|_| "invalid hex digit")?;
    Ok(Rgb8::new(r, g, b))
}

fn number_size_row(
    current: rollshot_image_document::NumberSize,
) -> iced::Element<'static, Message> {
    use iced::widget::{button, row, text};

    row![
        text("Size:"),
        button(text("S"))
            .on_press(Message::SetNumberSize(
                rollshot_image_document::NumberSize::Small
            ))
            .style(if current == rollshot_image_document::NumberSize::Small {
                button::primary
            } else {
                button::secondary
            }),
        button(text("M"))
            .on_press(Message::SetNumberSize(
                rollshot_image_document::NumberSize::Medium
            ))
            .style(if current == rollshot_image_document::NumberSize::Medium {
                button::primary
            } else {
                button::secondary
            }),
        button(text("L"))
            .on_press(Message::SetNumberSize(
                rollshot_image_document::NumberSize::Large
            ))
            .style(if current == rollshot_image_document::NumberSize::Large {
                button::primary
            } else {
                button::secondary
            }),
    ]
    .spacing(4)
    .into()
}

fn text_size_row(current: rollshot_image_document::TextSize) -> iced::Element<'static, Message> {
    use iced::widget::{row, text};

    row![
        text("Size:"),
        text_size_button("14", rollshot_image_document::TextSize::Px14, current),
        text_size_button("18", rollshot_image_document::TextSize::Px18, current),
        text_size_button("24", rollshot_image_document::TextSize::Px24, current),
        text_size_button("32", rollshot_image_document::TextSize::Px32, current),
    ]
    .spacing(4)
    .into()
}

fn text_size_button(
    label: &'static str,
    size: rollshot_image_document::TextSize,
    current: rollshot_image_document::TextSize,
) -> iced::Element<'static, Message> {
    use iced::widget::{button, text};

    button(text(label))
        .on_press(Message::SetTextSize(size))
        .style(if size == current {
            button::primary
        } else {
            button::secondary
        })
        .into()
}

fn color_button(label: &'static str, property: ColorProperty) -> Element<'static, Message> {
    use iced::widget::{button, text};

    button(text(label))
        .on_press(Message::OpenColorPicker(property))
        .into()
}

fn text_bg_toggle(has_bg: bool) -> iced::Element<'static, Message> {
    use iced::widget::{button, text};

    button(text(if has_bg { "BG On" } else { "BG Off" }))
        .on_press(Message::ToggleTextBackground)
        .into()
}

fn stroke_width(state: &ResultWorkspace, target: PropertyTarget) -> Option<f32> {
    match target {
        PropertyTarget::TwoPointTool(TwoPointKind::Line) => {
            Some(state.annotation_defaults.values.line.width)
        }
        PropertyTarget::TwoPointTool(TwoPointKind::Arrow) => {
            Some(state.annotation_defaults.values.arrow.width)
        }
        PropertyTarget::ShapeTool(kind) => {
            Some(state.annotation_defaults.values.shape(kind).stroke.width)
        }
        PropertyTarget::Annotation(id) => state
            .document
            .image
            .annotation(id)?
            .stroke_style()
            .map(|s| s.width),
        PropertyTarget::NumberTool | PropertyTarget::TextTool => None,
    }
}

fn stroke_controls(
    state: &ResultWorkspace,
    target: PropertyTarget,
) -> Option<Element<'static, Message>> {
    use iced::widget::{row, slider};

    let width = state
        .editor
        .properties
        .width
        .as_ref()
        .filter(|transaction| transaction.target == target)
        .map(|transaction| transaction.preview)
        .or_else(|| stroke_width(state, target))?;
    Some(
        row![
            color_button("Color", ColorProperty::StrokeColor),
            slider(1.0_f32..=16.0_f32, width, Message::PreviewStrokeWidth)
                .step(1.0_f32)
                .on_release(Message::ApplyStrokeWidth)
                .width(96),
        ]
        .spacing(8)
        .into(),
    )
}

/// Build the property controls row for the current tool/selection.
///
/// Returns `None` when the active tool has no associated properties (Redact,
/// OcrText, Select with no matching annotation). The caller wraps this in a
/// conditional row to keep the widget tree stable.
pub fn view(state: &ResultWorkspace) -> Option<Element<'_, Message>> {
    use iced::widget::row;

    let target = property_target(state)?;
    match target {
        PropertyTarget::NumberTool => {
            let defaults = &state.annotation_defaults.values.number;
            Some(
                row![
                    color_button("Color", ColorProperty::NumberAccent),
                    number_size_row(defaults.size),
                    iced::widget::text_input("Next", &state.editor.properties.next_number_input)
                        .on_input(Message::NextNumberInputChanged)
                        .on_submit(Message::CommitNextNumber)
                        .width(70),
                ]
                .spacing(8)
                .into(),
            )
        }
        PropertyTarget::TextTool => {
            let defaults = &state.annotation_defaults.values.text;
            let size = text_size_row(defaults.font_size);
            let bg = text_bg_toggle(defaults.background.is_some());
            let mut controls = row![
                color_button("Text color", ColorProperty::TextColor),
                size,
                bg
            ]
            .spacing(8);
            if defaults.background.is_some() {
                controls = controls.push(color_button("BG color", ColorProperty::TextBackground));
            }
            Some(controls.into())
        }
        PropertyTarget::TwoPointTool(_) => stroke_controls(state, target),
        PropertyTarget::ShapeTool(_) => stroke_controls(state, target),
        PropertyTarget::Annotation(id) => match state.document.image.annotation(id)? {
            Annotation::TwoPoint { .. } => stroke_controls(state, target),
            Annotation::NumberCallout { style, .. } => Some(
                row![
                    color_button("Color", ColorProperty::NumberAccent),
                    number_size_row(style.size)
                ]
                .spacing(8)
                .into(),
            ),
            Annotation::TextNote { style, .. } => {
                let size = text_size_row(style.font_size);
                let bg = text_bg_toggle(style.background.is_some());
                let mut controls = row![
                    color_button("Text color", ColorProperty::TextColor),
                    size,
                    bg
                ]
                .spacing(8);
                if style.background.is_some() {
                    controls =
                        controls.push(color_button("BG color", ColorProperty::TextBackground));
                }
                Some(controls.into())
            }
            _ => None,
        },
    }
}

/// Build an app-only preview clone of the selected annotation with the
/// current property transactions applied. Returns `None` when no preview is
/// active or the selection is not a styled annotation.
///
/// **Critical invariant:** The clone never enters the document or flatten
/// output — it is only used by `AnnotationCanvas` for live rendering.
pub fn preview_annotation(state: &ResultWorkspace) -> Option<Annotation> {
    let id = state.editor.selection?;
    let annotation = state.document.image.annotation(id)?.clone();
    match annotation {
        Annotation::TwoPoint {
            id,
            kind,
            start,
            end,
            mut style,
        } => {
            let mut changed = false;
            if let Some(tx) = state.editor.properties.color.as_ref() {
                if tx.target == PropertyTarget::Annotation(id)
                    && tx.property == ColorProperty::StrokeColor
                {
                    style.color = tx.preview;
                    changed = true;
                }
            }
            if let Some(tx) = state.editor.properties.width.as_ref() {
                if tx.target == PropertyTarget::Annotation(id) {
                    style.width = tx.preview;
                    changed = true;
                }
            }
            changed.then_some(Annotation::TwoPoint {
                id,
                kind,
                start,
                end,
                style,
            })
        }
        Annotation::NumberCallout {
            id,
            number,
            tip,
            bubble,
            style,
        } => {
            let tx = state.editor.properties.color.as_ref()?;
            if tx.target != PropertyTarget::Annotation(id)
                || !matches!(tx.property, ColorProperty::NumberAccent)
            {
                return None;
            }
            let mut s = style;
            s.accent = tx.preview;
            Some(Annotation::NumberCallout {
                id,
                number,
                tip,
                bubble,
                style: s,
            })
        }
        Annotation::TextNote {
            id,
            position,
            text,
            style,
        } => match state.editor.properties.color.as_ref()? {
            tx if tx.target == PropertyTarget::Annotation(id) => match tx.property {
                ColorProperty::TextColor => {
                    let mut s = style;
                    s.text_color = tx.preview;
                    Some(Annotation::TextNote {
                        id,
                        position,
                        text,
                        style: s,
                    })
                }
                ColorProperty::TextBackground => {
                    let mut s = style;
                    s.background = Some(tx.preview);
                    Some(Annotation::TextNote {
                        id,
                        position,
                        text,
                        style: s,
                    })
                }
                _ => None,
            },
            _ => None,
        },
        Annotation::OpaqueRedaction { .. } => None,
        Annotation::Shape {
            id,
            kind,
            bounds,
            stroke: _,
            fill: _,
        } => {
            let tx = state.editor.properties.shape_style.as_ref()?;
            if tx.id != id {
                return None;
            }
            Some(Annotation::Shape {
                id,
                kind,
                bounds,
                stroke: tx.preview_stroke,
                fill: tx.preview_fill,
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::result_workspace::document::ResultDocument;
    use image::{Rgba, RgbaImage};
    use rollshot_image_document::{ImagePoint, ImageRect};

    fn image() -> RgbaImage {
        RgbaImage::from_pixel(200, 200, Rgba([100, 150, 200, 255]))
    }

    fn workspace() -> ResultWorkspace {
        ResultWorkspace::new(ResultDocument::unsaved(image()), None)
    }

    fn add_number(state: &mut ResultWorkspace) -> AnnotationId {
        state
            .document
            .image
            .add_number_callout(ImagePoint::new(10.0, 10.0), ImagePoint::new(10.0, 10.0))
    }

    fn add_text(state: &mut ResultWorkspace) -> AnnotationId {
        state
            .document
            .image
            .add_text_note(ImagePoint::new(10.0, 10.0), "hello".into())
            .unwrap()
    }

    fn add_redaction(state: &mut ResultWorkspace) -> AnnotationId {
        state
            .document
            .image
            .add_redaction(ImageRect {
                x: 10.0,
                y: 10.0,
                width: 20.0,
                height: 20.0,
            })
            .unwrap()
    }

    #[test]
    fn selected_annotation_wins_over_tool_defaults_only_in_select_mode() {
        let mut state = workspace();
        let id = add_text(&mut state);
        state.editor.tool = Tool::Select;
        state.editor.selection = Some(id);
        assert_eq!(
            property_target(&state),
            Some(PropertyTarget::Annotation(id))
        );
        state.editor.tool = Tool::Text;
        assert_eq!(property_target(&state), Some(PropertyTarget::TextTool));
    }

    #[test]
    fn number_tool_returns_number_target() {
        let mut state = workspace();
        state.editor.tool = Tool::Number;
        assert_eq!(property_target(&state), Some(PropertyTarget::NumberTool));
    }

    #[test]
    fn creation_tools_target_independent_two_point_defaults() {
        let mut state = workspace();
        state.editor.tool = Tool::Line;
        assert_eq!(
            property_target(&state),
            Some(PropertyTarget::TwoPointTool(
                rollshot_image_document::TwoPointKind::Line
            ))
        );
        state.editor.tool = Tool::Arrow;
        assert_eq!(
            property_target(&state),
            Some(PropertyTarget::TwoPointTool(
                rollshot_image_document::TwoPointKind::Arrow
            ))
        );
    }

    #[test]
    fn redact_tool_yields_no_property_target() {
        let mut state = workspace();
        state.editor.tool = Tool::Redact;
        assert_eq!(property_target(&state), None);
    }

    #[test]
    fn select_with_no_selection_returns_none() {
        let mut state = workspace();
        state.editor.tool = Tool::Select;
        assert_eq!(property_target(&state), None);
    }

    #[test]
    fn select_with_selected_redaction_returns_none() {
        let mut state = workspace();
        let id = add_redaction(&mut state);
        state.editor.tool = Tool::Select;
        state.editor.selection = Some(id);
        assert_eq!(property_target(&state), None);
    }

    #[test]
    fn select_with_selected_number_returns_annotation_target() {
        let mut state = workspace();
        let id = add_number(&mut state);
        state.editor.tool = Tool::Select;
        state.editor.selection = Some(id);
        assert_eq!(
            property_target(&state),
            Some(PropertyTarget::Annotation(id))
        );
    }

    #[test]
    fn creation_tool_targets_ignore_selection() {
        let mut state = workspace();
        let id = add_text(&mut state);
        state.editor.tool = Tool::Number;
        state.editor.selection = Some(id);
        assert_eq!(property_target(&state), Some(PropertyTarget::NumberTool));
    }

    #[test]
    fn parse_hex_rgb_valid_with_hash() {
        assert_eq!(parse_hex_rgb("#FF00AA"), Ok(Rgb8::new(0xFF, 0x00, 0xAA)));
    }

    #[test]
    fn parse_hex_rgb_valid_without_hash() {
        assert_eq!(parse_hex_rgb("00FF00"), Ok(Rgb8::new(0x00, 0xFF, 0x00)));
    }

    #[test]
    fn parse_hex_rgb_valid_lowercase() {
        assert_eq!(parse_hex_rgb("#aabbcc"), Ok(Rgb8::new(0xAA, 0xBB, 0xCC)));
    }

    #[test]
    fn parse_hex_rgb_too_short() {
        assert!(parse_hex_rgb("#FFF").is_err());
    }

    #[test]
    fn parse_hex_rgb_too_long() {
        assert!(parse_hex_rgb("#FFFFFFF").is_err());
    }

    #[test]
    fn parse_hex_rgb_invalid_chars() {
        assert!(parse_hex_rgb("#GGHHII").is_err());
    }

    #[test]
    fn parse_hex_rgb_empty() {
        assert!(parse_hex_rgb("").is_err());
    }

    #[test]
    fn parse_hex_rgb_with_whitespace() {
        assert_eq!(
            parse_hex_rgb("  #112233  "),
            Ok(Rgb8::new(0x11, 0x22, 0x33))
        );
    }

    #[test]
    fn popup_is_exclusive() {
        let popup = Popup::ColorPicker;
        assert_eq!(popup, Popup::ColorPicker);
        assert_ne!(popup, Popup::CopyMenu);
    }

    #[test]
    fn color_transaction_clones_and_compares() {
        let tx = ColorTransaction {
            target: PropertyTarget::NumberTool,
            property: ColorProperty::NumberAccent,
            original: Rgb8::new(0, 0, 0),
            preview: Rgb8::new(255, 0, 0),
            hex: "#FF0000".into(),
        };
        let tx2 = tx.clone();
        assert_eq!(tx, tx2);
    }

    #[test]
    fn property_state_default_is_empty() {
        let ps = PropertyState::default();
        assert!(ps.color.is_none());
        assert!(ps.width.is_none());
        assert!(ps.next_number_input.is_empty());
        assert!(ps.focus.is_none());
        assert!(ps.popup.is_none());
    }

    // -- property view tests (Task 6) ----------------------------------------

    #[test]
    fn select_with_no_selection_has_no_properties() {
        let mut state = workspace();
        state.editor.tool = Tool::Select;
        state.editor.selection = None;
        assert_eq!(property_target(&state), None);
    }

    #[test]
    fn number_tool_shows_number_target() {
        let mut state = workspace();
        state.editor.tool = Tool::Number;
        assert_eq!(property_target(&state), Some(PropertyTarget::NumberTool));
    }

    #[test]
    fn text_tool_shows_text_target() {
        let mut state = workspace();
        state.editor.tool = Tool::Text;
        assert_eq!(property_target(&state), Some(PropertyTarget::TextTool));
    }

    #[test]
    fn redact_tool_shows_no_properties() {
        let mut state = workspace();
        state.editor.tool = Tool::Redact;
        assert_eq!(property_target(&state), None);
    }

    #[test]
    fn selected_number_annotation_shows_annotation_target() {
        let mut state = workspace();
        let id = add_number(&mut state);
        state.editor.tool = Tool::Select;
        state.editor.selection = Some(id);
        let target = property_target(&state);
        assert!(matches!(target, Some(PropertyTarget::Annotation(id2)) if id2 == id));
    }

    #[test]
    fn selected_text_annotation_shows_annotation_target() {
        let mut state = workspace();
        let id = add_text(&mut state);
        state.editor.tool = Tool::Select;
        state.editor.selection = Some(id);
        let target = property_target(&state);
        assert!(matches!(target, Some(PropertyTarget::Annotation(id2)) if id2 == id));
    }

    #[test]
    fn number_size_label_matches_variants() {
        assert_eq!(rollshot_image_document::NumberSize::ALL.len(), 3);
        assert_eq!(rollshot_image_document::NumberSize::Small.scale(), 0.75);
        assert_eq!(rollshot_image_document::NumberSize::Medium.scale(), 1.0);
        assert_eq!(rollshot_image_document::NumberSize::Large.scale(), 1.3);
    }

    #[test]
    fn text_size_label_matches_variants() {
        assert_eq!(rollshot_image_document::TextSize::ALL.len(), 4);
        assert!((rollshot_image_document::TextSize::Px14.pixels() - 14.0).abs() < f32::EPSILON);
        assert!((rollshot_image_document::TextSize::Px18.pixels() - 18.0).abs() < f32::EPSILON);
        assert!((rollshot_image_document::TextSize::Px24.pixels() - 24.0).abs() < f32::EPSILON);
        assert!((rollshot_image_document::TextSize::Px32.pixels() - 32.0).abs() < f32::EPSILON);
    }

    // -- shape style tests (Task 3) ------------------------------------------

    fn add_shape(
        state: &mut ResultWorkspace,
        kind: rollshot_image_document::ShapeKind,
    ) -> rollshot_image_document::AnnotationId {
        state
            .document
            .image
            .add_shape(
                kind,
                ImageRect {
                    x: 10.0,
                    y: 10.0,
                    width: 50.0,
                    height: 50.0,
                },
            )
            .unwrap()
    }

    #[test]
    fn rectangle_tool_resolves_to_shape_tool_target() {
        let mut state = workspace();
        state.editor.tool = Tool::Rectangle;
        assert_eq!(
            property_target(&state),
            Some(PropertyTarget::ShapeTool(
                rollshot_image_document::ShapeKind::Rectangle
            ))
        );
    }

    #[test]
    fn ellipse_tool_resolves_to_shape_tool_target() {
        let mut state = workspace();
        state.editor.tool = Tool::Ellipse;
        assert_eq!(
            property_target(&state),
            Some(PropertyTarget::ShapeTool(
                rollshot_image_document::ShapeKind::Ellipse
            ))
        );
    }

    #[test]
    fn selected_shape_resolves_to_annotation_target() {
        let mut state = workspace();
        let id = add_shape(&mut state, rollshot_image_document::ShapeKind::Rectangle);
        state.editor.tool = Tool::Select;
        state.editor.selection = Some(id);
        assert_eq!(
            property_target(&state),
            Some(PropertyTarget::Annotation(id))
        );
    }

    #[test]
    fn shape_preview_applies_transaction_stroke_and_fill() {
        let mut state = workspace();
        let id = add_shape(&mut state, rollshot_image_document::ShapeKind::Rectangle);
        state.editor.tool = Tool::Select;
        state.editor.selection = Some(id);
        let tx = ShapeStyleTransaction {
            id,
            kind: rollshot_image_document::ShapeKind::Rectangle,
            original_stroke: StrokeStyle::default(),
            original_fill: None,
            preview_stroke: StrokeStyle {
                width: 8.0,
                ..StrokeStyle::default()
            },
            preview_fill: Some(Rgb8::new(0, 255, 0)),
            remembered_fill_color: Rgb8::new(0xE5, 0x48, 0x4D),
        };
        state.editor.properties.shape_style = Some(tx);
        let preview = preview_annotation(&state).unwrap();
        match preview {
            Annotation::Shape { stroke, fill, .. } => {
                assert_eq!(stroke.width, 8.0);
                assert_eq!(fill, Some(Rgb8::new(0, 255, 0)));
            }
            _ => panic!("expected Shape"),
        }
    }

    #[test]
    fn shape_preview_none_when_ids_dont_match() {
        let mut state = workspace();
        let id = add_shape(&mut state, rollshot_image_document::ShapeKind::Rectangle);
        state.editor.tool = Tool::Select;
        state.editor.selection = Some(id);
        let tx = ShapeStyleTransaction {
            id: rollshot_image_document::AnnotationId(u64::MAX),
            kind: rollshot_image_document::ShapeKind::Rectangle,
            original_stroke: StrokeStyle::default(),
            original_fill: None,
            preview_stroke: StrokeStyle::default(),
            preview_fill: None,
            remembered_fill_color: Rgb8::new(0xE5, 0x48, 0x4D),
        };
        state.editor.properties.shape_style = Some(tx);
        assert!(preview_annotation(&state).is_none());
    }
}
