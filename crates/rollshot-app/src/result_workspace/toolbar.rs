#![allow(dead_code)]

use super::canvas::Tool;
use super::ResultWorkspace;

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
    pub const COPY: Self = Self {
        kind: ToolbarItemKind::Copy,
        label: "Copy",
        shortcut: "",
    };
    pub const COPY_SPLIT: Self = Self {
        kind: ToolbarItemKind::CopySplit,
        label: "\u{25BE}",
        shortcut: "",
    };
    pub const SAVE: Self = Self {
        kind: ToolbarItemKind::Save,
        label: "Save As",
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
        Tool::Redact => ToolbarItem {
            kind: ToolbarItemKind::Tool(Tool::Redact),
            label: "Redact",
            shortcut: "R",
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
    pub first_row: Vec<ToolbarItem>,
    pub visible_tools: Vec<Tool>,
    pub more: Vec<ToolbarItem>,
    pub more_active_tool: Option<(Tool, &'static str)>,
}

pub fn toolbar_model(state: &ResultWorkspace, width: f32) -> ToolbarModel {
    let density = density_for_width(width);

    let primary_tools = match density {
        ToolbarDensity::Wide | ToolbarDensity::Compact => {
            vec![Tool::Select, Tool::Number, Tool::Text, Tool::Redact]
        }
        ToolbarDensity::Narrow => vec![Tool::Select, Tool::Number, Tool::Text],
    };

    let mut overflow = vec![
        ToolbarItem::SMART_REDACTION,
        ToolbarItem::NAVIGATOR,
        ToolbarItem::REVEAL,
        ToolbarItem::EXPORT_BUG_REPORT,
    ];

    if density == ToolbarDensity::Narrow {
        overflow.insert(0, tool_item(Tool::Redact));
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
// Color picker math (pure functions)
// ---------------------------------------------------------------------------

use iced::{Point, Size};

pub fn sv_from_point(point: Point, size: Size) -> (f32, f32) {
    let s = (point.x / size.width).clamp(0.0, 1.0);
    let v = 1.0 - (point.y / size.height).clamp(0.0, 1.0);
    (s, v)
}

pub fn hue_from_x(x: f32, width: f32) -> f32 {
    (x / width).clamp(0.0, 1.0) * 360.0
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
        ResultWorkspace::new(ResultDocument::unsaved(image()), None)
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
    fn wide_shows_all_four_primary_tools() {
        let model = toolbar_model(&state(), 1100.0);
        assert_eq!(
            model.visible_tools,
            vec![Tool::Select, Tool::Number, Tool::Text, Tool::Redact]
        );
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
}
