use image::RgbaImage;

use super::viewport::{anchored_scroll, geometry_for, step_zoom, ZoomDirection, ZoomMode};
use iced::widget::scrollable;
use iced::{keyboard, mouse, Point, Size, Subscription, Task, Vector};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use super::{CloseDecision, InlineMessage, WHEEL_LINE_PX};

// ---------------------------------------------------------------------------
// Message enum
// ---------------------------------------------------------------------------

/// Messages produced by the Result Workspace UI.
#[derive(Debug, Clone)]
#[allow(clippy::enum_variant_names)]
pub enum Message {
    /// User requested window close (Close button, Esc, or window-manager close).
    RequestClose,
    /// User confirmed they want to discard unsaved changes.
    ConfirmDiscard,
    /// User chose to keep the window open despite unsaved changes.
    KeepUnsaved,
    /// User dismissed the inline success/error banner.
    DismissMessage,
    /// User pressed "Copy".
    Copy,
    /// User pressed "Copy Original" (unflattened source).
    CopyOriginal,
    /// Background clipboard write completed.
    CopyFinished(Result<(), String>),
    /// User pressed "Save As…".
    SaveAs,
    /// The async file-picker returned (None = cancelled).
    SavePathChosen(Option<PathBuf>),
    /// Background PNG write completed.
    SaveFinished(Result<PathBuf, String>),
    /// User pressed "Reveal".
    Reveal,
    /// Background reveal command completed.
    RevealFinished(Result<(), String>),
    /// Subscription tick for expiring success messages.
    Tick(Instant),
    /// Select an explicit zoom mode (fit modes, 100%, etc.).
    SetZoom(ZoomMode),
    /// Step the zoom in or out through the fixed steps.
    ZoomStep(ZoomDirection),
    /// The scrollable reported new bounds + absolute offset.
    ViewportChanged { bounds: Size, offset: Vector },
    /// Keyboard modifiers changed.
    ModifiersChanged(keyboard::Modifiers),
    /// Canvas pointer moved (scrollable-local position, from `mouse_area.on_move`).
    PointerMoved(Point),
    /// Click on the discard-modal scrim. No-op: present only so the scrim
    /// `mouse_area` captures the press and blocks the base layer.
    ModalScrimPressed,
    /// Wheel scrolled over the canvas.
    WheelScrolled(mouse::ScrollDelta),
    /// Select an annotation tool.
    SelectTool(super::canvas::Tool),
    /// Undo the last annotation edit.
    Undo,
    /// Redo the last undone annotation edit.
    Redo,
    /// Delete the currently selected annotation.
    #[allow(dead_code)]
    DeleteSelected,
    /// Escape priority: cancel draft → clear selection → request close.
    EscapePressed,
    /// Toggle the Navigator panel.
    ToggleNavigator,
    /// Toggle the Copy menu dropdown.
    ToggleCopyMenu,
}

// ---------------------------------------------------------------------------
// Payload helpers
// ---------------------------------------------------------------------------

/// The image Copy places on the clipboard: always the flattened document
/// (pixel-identical to the source when no annotations exist — spec §12.1).
pub(crate) fn copy_payload(state: &super::ResultWorkspace) -> RgbaImage {
    state.document.image.flatten()
}

pub(crate) fn copy_original_payload(state: &super::ResultWorkspace) -> RgbaImage {
    state.document.image.source().clone()
}

/// The image Save As writes: original bytes when no annotations exist,
/// otherwise the flattened document (spec §12.2).
pub(crate) fn save_payload(state: &super::ResultWorkspace) -> RgbaImage {
    if state.document.image.annotations().is_empty() {
        state.document.image.source().clone()
    } else {
        state.document.image.flatten()
    }
}

// ---------------------------------------------------------------------------
// Update
// ---------------------------------------------------------------------------

pub(crate) fn update(state: &mut super::ResultWorkspace, message: Message) -> Task<Message> {
    match message {
        Message::RequestClose => {
            commit_text_draft(state);
            match super::document::close_decision(&state.document, state.annotations_dirty()) {
                CloseDecision::Close => iced::exit(),
                CloseDecision::Confirm(prompt) => {
                    state.pending_discard = Some(prompt);
                    Task::none()
                }
            }
        }
        Message::ConfirmDiscard => iced::exit(),
        Message::KeepUnsaved => {
            state.pending_discard = None;
            Task::none()
        }
        Message::DismissMessage => {
            state.message = None;
            Task::none()
        }
        Message::Copy => {
            commit_text_draft(state);
            let result = super::actions::copy_image(&copy_payload(state));
            Task::done(Message::CopyFinished(result))
        }
        Message::CopyOriginal => {
            state.editor.copy_menu_open = false;
            commit_text_draft(state);
            let result = super::actions::copy_image(&copy_original_payload(state));
            Task::done(Message::CopyFinished(result))
        }
        Message::CopyFinished(Ok(())) => {
            state.message = Some(InlineMessage::success("Copied image".to_string()));
            Task::none()
        }
        Message::CopyFinished(Err(e)) => {
            state.message = Some(InlineMessage::Error(e));
            Task::none()
        }
        Message::SaveAs => {
            commit_text_draft(state);
            let default_dir = crate::storage::Platform::current()
                .and_then(crate::storage::default_output_dir)
                .unwrap_or_else(|_| PathBuf::from("."));
            let default_name = super::document::default_save_name(&state.document);
            Task::perform(
                super::actions::prompt_save_as(default_dir, default_name),
                Message::SavePathChosen,
            )
        }
        Message::SavePathChosen(Some(path)) => {
            let image = save_payload(state);
            Task::perform(
                async move { super::actions::write_save_as(&image, &path) },
                Message::SaveFinished,
            )
        }
        Message::SavePathChosen(None) => Task::none(),
        Message::SaveFinished(result) => {
            state.apply_save_as(result.map(Some));
            Task::none()
        }
        Message::Reveal => {
            let Some(path) = state.document.reveal_path().map(Path::to_path_buf) else {
                return Task::none();
            };
            Task::done(Message::RevealFinished(super::actions::reveal(&path)))
        }
        Message::RevealFinished(Ok(())) => Task::none(),
        Message::RevealFinished(Err(e)) => {
            state.message = Some(InlineMessage::Error(e));
            Task::none()
        }
        Message::Tick(now) => {
            if let Some(msg) = &state.message {
                if msg.expiry().is_some_and(|deadline| now >= deadline) {
                    state.message = None;
                }
            }
            Task::none()
        }
        Message::SetZoom(mode) => {
            state.viewport.zoom = mode;
            Task::none()
        }
        Message::ZoomStep(dir) => {
            let next = step_zoom(state.viewport.zoom, dir);
            apply_zoom_at_pointer(state, next)
        }
        Message::ViewportChanged { bounds, offset } => {
            state.apply_viewport_bounds(bounds);
            state.viewport.scroll_offset = offset;
            Task::none()
        }
        Message::ModifiersChanged(modifiers) => {
            state.modifiers = modifiers;
            Task::none()
        }
        Message::PointerMoved(position) => {
            state.pointer_position = position;
            Task::none()
        }
        Message::ModalScrimPressed => Task::none(),
        Message::WheelScrolled(delta) => handle_wheel(state, delta),
        Message::SelectTool(tool) => {
            commit_text_draft(state);
            state.editor.tool = tool;
            state.editor.drag = None;
            Task::none()
        }
        Message::Undo => {
            commit_text_draft(state);
            let _ = state.document.image.undo();
            prune_stale_selection(state);
            Task::none()
        }
        Message::Redo => {
            commit_text_draft(state);
            let _ = state.document.image.redo();
            prune_stale_selection(state);
            Task::none()
        }
        Message::DeleteSelected => {
            if state.editor.text_draft.is_some() {
                return Task::none();
            }
            if let Some(id) = state.editor.selection.take() {
                let _ = state.document.image.delete_annotation(id);
            }
            Task::none()
        }
        Message::EscapePressed => {
            if state.editor.copy_menu_open {
                state.editor.copy_menu_open = false;
            } else if state.editor.text_draft.is_some() {
                state.editor.text_draft = None;
            } else if state.editor.drag.is_some() {
                state.editor.drag = None;
            } else if state.editor.selection.is_some() {
                state.editor.selection = None;
            } else {
                return update(state, Message::RequestClose);
            }
            Task::none()
        }
        Message::ToggleNavigator => {
            state.editor.navigator_open = !state.editor.navigator_open;
            Task::none()
        }
        Message::ToggleCopyMenu => {
            state.editor.copy_menu_open = !state.editor.copy_menu_open;
            Task::none()
        }
    }
}

/// Drop a selection whose annotation no longer exists (spec §15).
fn prune_stale_selection(state: &mut super::ResultWorkspace) {
    if let Some(id) = state.editor.selection {
        if state.document.image.annotation(id).is_none() {
            state.editor.selection = None;
        }
    }
}

/// Commit a valid inline text draft, or cancel an invalid one (spec §15).
/// Full implementation lands with the text editor task; until then:
fn commit_text_draft(state: &mut super::ResultWorkspace) {
    let _ = state;
}

/// The platform zoom modifier: Cmd on macOS, Ctrl on Linux.
fn zoom_modifier_held(modifiers: keyboard::Modifiers) -> bool {
    #[cfg(target_os = "macos")]
    {
        modifiers.command()
    }
    #[cfg(not(target_os = "macos"))]
    {
        modifiers.control()
    }
}

/// Apply a new zoom mode while keeping the image point under the pointer fixed,
/// then push the resulting scroll offset to the scrollable.
fn apply_zoom_at_pointer(state: &mut super::ResultWorkspace, next: ZoomMode) -> Task<Message> {
    let image = state.original_size();
    let viewport = state.viewport_bounds;
    let old_geometry = geometry_for(state.viewport.zoom, image, viewport);
    let new_geometry = geometry_for(next, image, viewport);
    let new_offset = anchored_scroll(
        state.viewport.scroll_offset,
        state.pointer_position,
        old_geometry,
        new_geometry,
    );
    state.viewport.zoom = next;
    state.viewport.scroll_offset = new_offset;
    iced::widget::operation::scroll_to(
        state.scrollable_id.clone(),
        scrollable::AbsoluteOffset {
            x: new_offset.x,
            y: new_offset.y,
        },
    )
}

/// Route a wheel event: zoom modifier → pointer-anchored zoom; Shift →
/// horizontal pan; otherwise vertical pan.
fn handle_wheel(state: &mut super::ResultWorkspace, delta: mouse::ScrollDelta) -> Task<Message> {
    let (dx, dy) = scroll_delta_pixels(delta);
    if zoom_modifier_held(state.modifiers) {
        let dir = if dy > 0.0 {
            ZoomDirection::In
        } else {
            ZoomDirection::Out
        };
        let next = step_zoom(state.viewport.zoom, dir);
        return apply_zoom_at_pointer(state, next);
    }

    let offset = if state.modifiers.shift() {
        // Shift maps vertical wheel travel to horizontal panning.
        scrollable::AbsoluteOffset { x: -dy, y: 0.0 }
    } else {
        scrollable::AbsoluteOffset { x: -dx, y: -dy }
    };
    iced::widget::operation::scroll_by(state.scrollable_id.clone(), offset)
}

fn scroll_delta_pixels(delta: mouse::ScrollDelta) -> (f32, f32) {
    match delta {
        mouse::ScrollDelta::Lines { x, y } => (x * WHEEL_LINE_PX, y * WHEEL_LINE_PX),
        mouse::ScrollDelta::Pixels { x, y } => (x, y),
    }
}

// ---------------------------------------------------------------------------
// Subscription
// ---------------------------------------------------------------------------

pub(crate) fn subscription(state: &super::ResultWorkspace) -> Subscription<Message> {
    let mut subs = vec![
        iced::window::close_requests().map(|_id| Message::RequestClose),
        iced::event::listen_with(|event, _status, _window| match event {
            iced::Event::Keyboard(keyboard::Event::ModifiersChanged(m)) => {
                Some(Message::ModifiersChanged(m))
            }
            iced::Event::Keyboard(keyboard::Event::KeyPressed {
                key: keyboard::Key::Named(keyboard::key::Named::Escape),
                ..
            }) => Some(Message::EscapePressed),
            // Pointer position for pointer-anchored zoom comes solely from the
            // canvas `mouse_area.on_move` (scrollable-local space, which
            // `anchored_scroll` expects). The global window-relative
            // `CursorMoved` event is intentionally NOT routed here: feeding it
            // into `pointer_position` would mix coordinate spaces and anchor
            // zoom at the wrong point.
            _ => None,
        }),
    ];

    // Only run the expiry timer while a success message has a live expiry.
    if state
        .message
        .as_ref()
        .and_then(InlineMessage::expiry)
        .is_some()
    {
        subs.push(iced::time::every(Duration::from_millis(250)).map(Message::Tick));
    }

    Subscription::batch(subs)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use iced::Size as IcedSize;
    use image::Rgba;
    use rollshot_image_document::{ImagePoint, ImageRect};

    fn image() -> image::RgbaImage {
        image::RgbaImage::from_pixel(2, 2, Rgba([100, 150, 200, 255]))
    }

    fn workspace() -> super::super::ResultWorkspace {
        super::super::ResultWorkspace::new(
            super::super::document::ResultDocument::unsaved(image()),
            None,
        )
    }

    fn unsaved_workspace() -> super::super::ResultWorkspace {
        super::super::ResultWorkspace::new(
            super::super::document::ResultDocument::unsaved(image()),
            None,
        )
    }

    fn saved_workspace() -> super::super::ResultWorkspace {
        super::super::ResultWorkspace::new(
            super::super::document::ResultDocument::saved(
                image(),
                std::path::PathBuf::from("/tmp/result.png"),
            ),
            None,
        )
    }

    // -- payload routing tests (Task 15) --------------------------------------

    #[test]
    fn copy_flattens_annotations_and_does_not_clear_dirty() {
        let mut state = unsaved_workspace();
        state
            .document
            .image
            .add_redaction(ImageRect {
                x: 0.0,
                y: 0.0,
                width: 1.0,
                height: 1.0,
            })
            .unwrap();
        assert!(state.annotations_dirty());
        let flattened = copy_payload(&state);
        assert_ne!(
            flattened.get_pixel(0, 0).0,
            state.document.image.source().get_pixel(0, 0).0,
            "copy payload is the flattened image"
        );
        assert!(
            state.annotations_dirty(),
            "spec §12.1: copy never clears dirty"
        );
    }

    #[test]
    fn copy_original_payload_is_the_source() {
        let mut state = unsaved_workspace();
        state
            .document
            .image
            .add_redaction(ImageRect {
                x: 0.0,
                y: 0.0,
                width: 1.0,
                height: 1.0,
            })
            .unwrap();
        let original = copy_original_payload(&state);
        assert_eq!(original.as_raw(), state.document.image.source().as_raw());
    }

    #[test]
    fn save_payload_is_source_without_annotations_and_flatten_with() {
        let mut state = unsaved_workspace();
        assert_eq!(
            save_payload(&state).as_raw(),
            state.document.image.source().as_raw(),
            "spec §12.2: no annotations → original bytes"
        );
        state
            .document
            .image
            .add_number_callout(ImagePoint::new(1.0, 1.0), ImagePoint::new(1.0, 1.0));
        assert_ne!(
            save_payload(&state).as_raw(),
            state.document.image.source().as_raw()
        );
    }

    /// The platform zoom modifier (Ctrl on Linux / Cmd on macOS), used to drive
    /// the zoom branch of `WheelScrolled` in tests.
    fn zoom_mods() -> keyboard::Modifiers {
        #[cfg(target_os = "macos")]
        {
            keyboard::Modifiers::COMMAND
        }
        #[cfg(not(target_os = "macos"))]
        {
            keyboard::Modifiers::CTRL
        }
    }

    // -- status controls (Task 5) --------------------------------------------

    #[test]
    fn fit_height_button_selects_fit_height() {
        let mut state = workspace();
        let _ = update(&mut state, Message::SetZoom(ZoomMode::FitHeight));
        assert_eq!(state.viewport.zoom, ZoomMode::FitHeight);
    }

    #[test]
    fn set_zoom_selects_each_fit_mode() {
        for mode in [
            ZoomMode::FitWidth,
            ZoomMode::FitWindow,
            ZoomMode::FitHeight,
            ZoomMode::ActualSize,
        ] {
            let mut state = workspace();
            let _ = update(&mut state, Message::SetZoom(mode));
            assert_eq!(state.viewport.zoom, mode);
        }
    }

    #[test]
    fn zoom_step_in_and_out_via_update() {
        let mut state = workspace();
        state.viewport.zoom = ZoomMode::Custom(100);
        let _ = update(&mut state, Message::ZoomStep(ZoomDirection::In));
        assert_eq!(state.viewport.zoom, ZoomMode::Custom(125));
        let _ = update(&mut state, Message::ZoomStep(ZoomDirection::Out));
        assert_eq!(state.viewport.zoom, ZoomMode::Custom(100));
    }

    // -- viewport ------------------------------------------------------------

    #[test]
    fn viewport_changed_records_bounds_and_offset() {
        let mut state = workspace();
        let _ = update(
            &mut state,
            Message::ViewportChanged {
                bounds: Size::new(800.0, 600.0),
                offset: Vector::new(20.0, 30.0),
            },
        );
        assert_eq!(state.viewport_bounds, Size::new(800.0, 600.0));
        assert_eq!(state.viewport.scroll_offset, Vector::new(20.0, 30.0));
    }

    // -- pointer / modifiers tracking ----------------------------------------

    #[test]
    fn pointer_and_modifiers_are_tracked() {
        let mut state = workspace();
        let _ = update(&mut state, Message::PointerMoved(Point::new(12.0, 34.0)));
        assert_eq!(state.pointer_position, Point::new(12.0, 34.0));

        let mods = keyboard::Modifiers::SHIFT;
        let _ = update(&mut state, Message::ModifiersChanged(mods));
        assert_eq!(state.modifiers, mods);
    }

    // -- window-close routing ------------------------------------------------

    #[test]
    fn operating_system_close_uses_unsaved_close_confirmation() {
        let mut state = unsaved_workspace();
        let _ = update(&mut state, Message::RequestClose);
        assert!(state.pending_discard.is_some());
    }

    #[test]
    fn saved_close_does_not_confirm_discard() {
        let mut state = saved_workspace();
        let _ = update(&mut state, Message::RequestClose);
        assert!(state.pending_discard.is_none());
    }

    #[test]
    fn confirm_discard_then_keep_unsaved_transitions() {
        let mut state = unsaved_workspace();
        let _ = update(&mut state, Message::RequestClose);
        assert!(state.pending_discard.is_some());
        let _ = update(&mut state, Message::KeepUnsaved);
        assert!(state.pending_discard.is_none());
    }

    // -- message expiry ------------------------------------------------------

    #[test]
    fn tick_expires_success_message_but_keeps_errors() {
        let mut state = workspace();
        state.message = Some(InlineMessage::Success {
            text: "Copied image".to_string(),
            expires_at: Instant::now() - Duration::from_secs(1),
        });
        let _ = update(&mut state, Message::Tick(Instant::now()));
        assert!(state.message.is_none());

        state.message = Some(InlineMessage::Error("boom".to_string()));
        let _ = update(&mut state, Message::Tick(Instant::now()));
        assert!(matches!(state.message, Some(InlineMessage::Error(_))));
    }

    #[test]
    fn dismiss_message_clears_it() {
        let mut state = workspace();
        state.message = Some(InlineMessage::Error("boom".to_string()));
        let _ = update(&mut state, Message::DismissMessage);
        assert!(state.message.is_none());
    }

    // -- wheel routing (Task 5 follow-up) ------------------------------------

    #[test]
    fn wheel_with_zoom_modifier_zooms_and_leaves_scroll_routing() {
        let mut state = workspace();
        // Give the canvas a real viewport + a baseline custom zoom so the
        // zoom branch produces an observable stepped change.
        state.apply_viewport_bounds(IcedSize::new(800.0, 600.0));
        state.viewport.zoom = ZoomMode::Custom(100);
        let _ = update(&mut state, Message::ModifiersChanged(zoom_mods()));

        // Positive wheel travel with the zoom modifier → zoom IN one step.
        let _ = update(
            &mut state,
            Message::WheelScrolled(mouse::ScrollDelta::Lines { x: 0.0, y: 1.0 }),
        );
        assert_eq!(state.viewport.zoom, ZoomMode::Custom(125));

        // Negative wheel travel → zoom OUT one step.
        let _ = update(
            &mut state,
            Message::WheelScrolled(mouse::ScrollDelta::Lines { x: 0.0, y: -1.0 }),
        );
        assert_eq!(state.viewport.zoom, ZoomMode::Custom(100));
    }

    #[test]
    fn wheel_with_shift_pans_horizontally_without_zoom() {
        let mut state = workspace();
        state.viewport.zoom = ZoomMode::Custom(150);
        let before = state.viewport.zoom;
        let _ = update(
            &mut state,
            Message::ModifiersChanged(keyboard::Modifiers::SHIFT),
        );
        // Shift maps vertical wheel travel to horizontal panning; zoom is
        // unchanged. (The offset itself is applied by the scrollable operation;
        // here we assert the routing did not touch zoom.)
        let _ = update(
            &mut state,
            Message::WheelScrolled(mouse::ScrollDelta::Lines { x: 0.0, y: 3.0 }),
        );
        assert_eq!(state.viewport.zoom, before);
        // Confirm the horizontal-pan branch is selected: it must not be read as
        // a zoom modifier on this platform.
        assert!(!zoom_modifier_held(state.modifiers));
        assert!(state.modifiers.shift());
    }

    #[test]
    fn plain_wheel_pans_vertically_without_zoom() {
        let mut state = workspace();
        state.viewport.zoom = ZoomMode::Custom(150);
        let before = state.viewport.zoom;
        // No modifiers held.
        let _ = update(
            &mut state,
            Message::WheelScrolled(mouse::ScrollDelta::Lines { x: 0.0, y: 2.0 }),
        );
        assert_eq!(state.viewport.zoom, before);
        assert!(!zoom_modifier_held(state.modifiers));
        assert!(!state.modifiers.shift());
    }

    // -- editor state, tools, undo/redo/delete/escape (Task 16) ---------------

    #[test]
    fn select_is_the_default_tool_and_tools_switch() {
        let mut state = unsaved_workspace();
        assert_eq!(state.editor.tool, super::super::canvas::Tool::Select);
        let _ = update(
            &mut state,
            Message::SelectTool(super::super::canvas::Tool::Number),
        );
        assert_eq!(state.editor.tool, super::super::canvas::Tool::Number);
    }

    #[test]
    fn switching_tools_preserves_viewport() {
        let mut state = unsaved_workspace();
        state.viewport.zoom = ZoomMode::Custom(150);
        state.viewport.scroll_offset = Vector::new(11.0, 22.0);
        let _ = update(
            &mut state,
            Message::SelectTool(super::super::canvas::Tool::Redact),
        );
        assert_eq!(state.viewport.zoom, ZoomMode::Custom(150));
        assert_eq!(state.viewport.scroll_offset, Vector::new(11.0, 22.0));
    }

    #[test]
    fn undo_redo_messages_drive_the_document() {
        let mut state = unsaved_workspace();
        state
            .document
            .image
            .add_number_callout(ImagePoint::new(1.0, 1.0), ImagePoint::new(1.0, 1.0));
        let _ = update(&mut state, Message::Undo);
        assert!(state.document.image.annotations().is_empty());
        let _ = update(&mut state, Message::Redo);
        assert_eq!(state.document.image.annotations().len(), 1);
    }

    #[test]
    fn delete_removes_the_selected_annotation_and_clears_selection() {
        let mut state = unsaved_workspace();
        let id = state
            .document
            .image
            .add_number_callout(ImagePoint::new(1.0, 1.0), ImagePoint::new(1.0, 1.0));
        state.editor.selection = Some(id);
        let _ = update(&mut state, Message::DeleteSelected);
        assert!(state.document.image.annotations().is_empty());
        assert_eq!(state.editor.selection, None);
    }

    #[test]
    fn escape_priority_draft_then_selection_then_close() {
        let mut state = unsaved_workspace();
        let id = state
            .document
            .image
            .add_number_callout(ImagePoint::new(1.0, 1.0), ImagePoint::new(1.0, 1.0));
        state.editor.selection = Some(id);
        state.editor.drag = Some(super::super::canvas::DragState::CreateRedaction {
            anchor: ImagePoint::new(0.0, 0.0),
            current: ImagePoint::new(1.0, 1.0),
        });

        let _ = update(&mut state, Message::EscapePressed);
        assert!(state.editor.drag.is_none(), "1st Esc cancels the draft");
        assert_eq!(state.editor.selection, Some(id), "selection survives");

        let _ = update(&mut state, Message::EscapePressed);
        assert_eq!(state.editor.selection, None, "2nd Esc clears selection");
        assert!(state.pending_discard.is_none());

        let _ = update(&mut state, Message::EscapePressed);
        assert!(
            state.pending_discard.is_some(),
            "3rd Esc requests close (unsaved)"
        );
    }

    #[test]
    fn undo_after_undo_clears_selection_of_removed_annotation() {
        let mut state = unsaved_workspace();
        let id = state
            .document
            .image
            .add_number_callout(ImagePoint::new(1.0, 1.0), ImagePoint::new(1.0, 1.0));
        state.editor.selection = Some(id);
        let _ = update(&mut state, Message::Undo);
        assert_eq!(state.editor.selection, None, "stale selection cleared");
    }

    // -- copy menu (Task 17) --------------------------------------------------

    #[test]
    fn copy_menu_toggles_and_copy_original_closes_it() {
        let mut state = unsaved_workspace();
        let _ = update(&mut state, Message::ToggleCopyMenu);
        assert!(state.editor.copy_menu_open);
        let _ = update(&mut state, Message::CopyOriginal);
        assert!(
            !state.editor.copy_menu_open,
            "choosing an item closes the menu"
        );
    }
}
