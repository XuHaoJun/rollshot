use super::super::{Message, ResultWorkspace};

/// Placeholder; Task 6 fills in the real layout. Takes `&ResultWorkspace` so
/// the real layout can reuse the existing `canvas_view` machinery (which
/// reads viewport/editor/handle from `ResultWorkspace`, not `WorkbenchState`).
pub fn workbench_view<'a>(state: &'a ResultWorkspace) -> iced::Element<'a, Message> {
    let _ = state;
    iced::widget::text("Smart Redaction (work in progress)").into()
}
