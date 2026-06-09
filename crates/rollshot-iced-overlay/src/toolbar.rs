use crate::workspace::{CropRect, WorkspacePhase};
use iced::widget::{button, container, row, text, tooltip};
use iced::{Element, Point};
use rollshot_capture::CaptureMode;
use rollshot_overlay_core::chrome_placement::Rect;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolbarAction {
    ScreenshotMode,
    ScrollingMode,
    Finish,
    Cancel,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub enum ToolbarMessage {
    Action(ToolbarAction),
    DragStart(Point),
    DragMove(Point),
    DragEnd,
}

pub const TOOLBAR_WIDTH: f32 = 360.0;
pub const TOOLBAR_HEIGHT: f32 = 48.0;

pub fn actions_for(phase: WorkspacePhase) -> Vec<ToolbarAction> {
    match phase {
        WorkspacePhase::Selecting => vec![
            ToolbarAction::ScreenshotMode,
            ToolbarAction::ScrollingMode,
            ToolbarAction::Cancel,
        ],
        WorkspacePhase::Selected => vec![
            ToolbarAction::ScreenshotMode,
            ToolbarAction::ScrollingMode,
            ToolbarAction::Finish,
            ToolbarAction::Cancel,
        ],
        WorkspacePhase::ScrollingCapture => vec![
            ToolbarAction::ScreenshotMode,
            ToolbarAction::ScrollingMode,
            ToolbarAction::Finish,
            ToolbarAction::Cancel,
        ],
    }
}

pub fn finish_drag(toolbar: Rect, viewport: Rect) -> CropRect {
    let x = toolbar
        .x
        .clamp(viewport.x, viewport.x + viewport.width - toolbar.width);
    let y = toolbar
        .y
        .clamp(viewport.y, viewport.y + viewport.height - toolbar.height);
    CropRect {
        x,
        y,
        width: toolbar.width,
        height: toolbar.height,
    }
}

fn action_label(action: ToolbarAction) -> &'static str {
    match action {
        ToolbarAction::ScreenshotMode => "📷",
        ToolbarAction::ScrollingMode => "📜",
        ToolbarAction::Finish => "✓",
        ToolbarAction::Cancel => "✕",
    }
}

fn action_tooltip(action: ToolbarAction) -> &'static str {
    match action {
        ToolbarAction::ScreenshotMode => "Screenshot Mode",
        ToolbarAction::ScrollingMode => "Scrolling Mode",
        ToolbarAction::Finish => "Finish Capture",
        ToolbarAction::Cancel => "Cancel",
    }
}

fn action_style_fn(
    action: ToolbarAction,
    active_mode: CaptureMode,
) -> impl Fn(&iced::Theme, button::Status) -> button::Style {
    let is_active = matches!(
        (action, active_mode),
        (ToolbarAction::ScreenshotMode, CaptureMode::Screenshot)
            | (ToolbarAction::ScrollingMode, CaptureMode::Scrolling)
    );

    move |_theme, _status| {
        if is_active {
            button::Style {
                background: Some(iced::Background::Color(iced::Color::from_rgba(
                    0.2, 0.6, 1.0, 0.8,
                ))),
                text_color: iced::Color::WHITE,
                border: iced::Border {
                    color: iced::Color::from_rgba(0.3, 0.7, 1.0, 1.0),
                    width: 1.0,
                    radius: 4.0.into(),
                },
                ..Default::default()
            }
        } else {
            button::Style {
                background: Some(iced::Background::Color(iced::Color::from_rgba(
                    0.15, 0.15, 0.15, 0.9,
                ))),
                text_color: iced::Color::WHITE,
                border: iced::Border {
                    color: iced::Color::from_rgba(0.3, 0.3, 0.3, 1.0),
                    width: 1.0,
                    radius: 4.0.into(),
                },
                ..Default::default()
            }
        }
    }
}

pub fn render_toolbar<'a, Message>(
    phase: WorkspacePhase,
    active_mode: CaptureMode,
    on_action: impl Fn(ToolbarAction) -> Message + 'a,
    on_drag_start: Message,
    on_drag_end: Message,
) -> Element<'a, Message>
where
    Message: Clone + 'a,
{
    let actions = actions_for(phase);
    let mut toolbar_row = row![].spacing(4).align_y(iced::Alignment::Center);

    for action in actions {
        let label = action_label(action);
        let tooltip_text = action_tooltip(action);
        let style_fn = action_style_fn(action, active_mode);

        let btn = button(text(label).size(16))
            .style(style_fn)
            .padding(8)
            .on_press(on_action(action));

        let tooltip_btn =
            tooltip(btn, tooltip_text, tooltip::Position::Top).style(|_theme| container::Style {
                text_color: Some(iced::Color::from_rgba(0.9, 0.9, 0.9, 1.0)),
                ..Default::default()
            });

        toolbar_row = toolbar_row.push(tooltip_btn);
    }

    let drag_handle =
        iced::widget::mouse_area(container(text("⋮⋮").size(14)).padding(8).style(|_theme| {
            container::Style {
                background: Some(iced::Background::Color(iced::Color::from_rgba(
                    0.1, 0.1, 0.1, 0.8,
                ))),
                text_color: Some(iced::Color::from_rgba(0.6, 0.6, 0.6, 1.0)),
                border: iced::Border {
                    color: iced::Color::from_rgba(0.3, 0.3, 0.3, 1.0),
                    width: 1.0,
                    radius: 4.0.into(),
                },
                ..Default::default()
            }
        }))
        .on_press(on_drag_start)
        .on_release(on_drag_end);

    container(
        row![drag_handle, toolbar_row]
            .spacing(8)
            .align_y(iced::Alignment::Center),
    )
    .padding(8)
    .style(|_theme| container::Style {
        background: Some(iced::Background::Color(iced::Color::from_rgba(
            0.1, 0.1, 0.1, 0.95,
        ))),
        border: iced::Border {
            color: iced::Color::from_rgba(0.4, 0.4, 0.4, 1.0),
            width: 1.0,
            radius: 8.0.into(),
        },
        ..Default::default()
    })
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workspace::WorkspacePhase;
    use rollshot_overlay_core::chrome_placement::Rect;

    #[test]
    fn selected_toolbar_contains_finish_but_no_output_actions() {
        assert_eq!(
            actions_for(WorkspacePhase::Selected),
            vec![
                ToolbarAction::ScreenshotMode,
                ToolbarAction::ScrollingMode,
                ToolbarAction::Finish,
                ToolbarAction::Cancel,
            ]
        );
    }

    #[test]
    fn scrolling_toolbar_includes_finish() {
        assert!(actions_for(WorkspacePhase::ScrollingCapture).contains(&ToolbarAction::Finish));
    }

    #[test]
    fn drag_is_clamped_to_viewport_bounds() {
        let clamped = finish_drag(
            Rect::new(990.0, 790.0, TOOLBAR_WIDTH, TOOLBAR_HEIGHT),
            Rect::new(0.0, 0.0, 1000.0, 800.0),
        );
        assert_eq!(clamped.x, 640.0);
        assert_eq!(clamped.y, 752.0);
        assert_eq!(clamped.width, TOOLBAR_WIDTH);
        assert_eq!(clamped.height, TOOLBAR_HEIGHT);
    }
}
