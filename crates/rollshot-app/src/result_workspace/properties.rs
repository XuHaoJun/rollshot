use rollshot_image_document::{Annotation, AnnotationId, Rgb8};

use super::canvas::Tool;
use super::ResultWorkspace;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PropertyTarget {
    NumberTool,
    TextTool,
    Annotation(AnnotationId),
}

#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorProperty {
    NumberAccent,
    TextColor,
    TextBackground,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColorTransaction {
    pub target: PropertyTarget,
    pub property: ColorProperty,
    pub original: Rgb8,
    pub preview: Rgb8,
    pub hex: String,
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
        Tool::Select => {
            state
                .editor
                .selection
                .and_then(|id| match state.document.image.annotation(id) {
                    Some(Annotation::NumberCallout { .. } | Annotation::TextNote { .. }) => {
                        Some(PropertyTarget::Annotation(id))
                    }
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

/// Build an app-only preview clone of the selected annotation with the
/// current color transaction applied. Returns `None` when no preview is
/// active or the selection is not a styled annotation.
///
/// **Critical invariant:** The clone never enters the document or flatten
/// output — it is only used by `AnnotationCanvas` for live rendering.
pub fn preview_annotation(state: &ResultWorkspace) -> Option<Annotation> {
    let tx = state.editor.properties.color.as_ref()?;
    let id = match tx.target {
        PropertyTarget::Annotation(id) => id,
        _ => return None,
    };
    let annotation = state.document.image.annotation(id)?.clone();
    match annotation {
        Annotation::NumberCallout {
            id,
            number,
            tip,
            bubble,
            style,
        } => {
            if !matches!(tx.property, ColorProperty::NumberAccent) {
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
        } => match tx.property {
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
        Annotation::OpaqueRedaction { .. } => None,
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
        assert!(ps.next_number_input.is_empty());
        assert!(ps.focus.is_none());
        assert!(ps.popup.is_none());
    }
}
