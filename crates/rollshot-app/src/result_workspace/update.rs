use image::RgbaImage;

use super::viewport::{anchored_scroll, geometry_for, step_zoom, ZoomDirection, ZoomMode};
use iced::widget::scrollable;
use iced::{keyboard, mouse, Point, Size, Subscription, Task, Vector};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use super::canvas::{
    dragged_annotation, DragState, EditorState, TextDraft, Tool, DOUBLE_CLICK_SLOP_SCREEN,
    DOUBLE_CLICK_WINDOW_MS, HIT_TOLERANCE_SCREEN,
};
use super::{CloseDecision, InlineMessage, WHEEL_LINE_PX};
use rollshot_image_document::{
    hit_test_annotation, Annotation, AnnotationId, Hit, HitPart, ImageDocument, ImagePoint,
    ImageRect,
};

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
    CopyFinished {
        result: Result<(), String>,
        safe_output: bool,
    },
    /// User pressed "Save As…".
    SaveAs,
    /// The async file-picker returned (None = cancelled).
    SavePathChosen(Option<PathBuf>),
    /// Background PNG write completed.
    SaveFinished {
        result: Result<PathBuf, String>,
        saved_state_id: u64,
        safe_output: bool,
    },
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
    /// Jump to an annotation via the Navigator.
    NavigatorJump(AnnotationId),
    /// Toggle the Copy menu dropdown.
    ToggleCopyMenu,
    /// Canvas pointer pressed (image-space coordinate).
    CanvasPressed(rollshot_image_document::ImagePoint),
    /// Canvas pointer moved (image-space coordinate).
    CanvasMoved(rollshot_image_document::ImagePoint),
    /// Canvas pointer released (image-space coordinate).
    CanvasReleased(rollshot_image_document::ImagePoint),
    /// Inline text editor action.
    TextDraftAction(iced::widget::text_editor::Action),
    /// Commit the inline text editor draft.
    CommitTextDraft,
    /// User confirmed the pending unredacted-action dialog.
    ConfirmUnredactedAction,
    /// User cancelled the pending unredacted-action dialog.
    CancelUnredactedAction,
    /// Smart Redaction toolbar button pressed.
    SmartRedaction,
    /// Messages forwarded from the workbench sub-state.
    #[allow(dead_code)] // SP6 scaffolding: constructed by later tasks
    Workbench(super::workbench::WorkbenchMessage),
}

// ---------------------------------------------------------------------------
// Gesture helpers
// ---------------------------------------------------------------------------

fn current_scale(state: &super::ResultWorkspace) -> f32 {
    geometry_for(
        state.viewport.zoom,
        state.original_size(),
        state.viewport_bounds,
    )
    .scale
}

fn grab_offset(annotation: &Annotation, part: HitPart, point: ImagePoint) -> (f32, f32) {
    match (annotation, part) {
        (Annotation::TextNote { position, .. }, HitPart::Body) => {
            (point.x - position.x, point.y - position.y)
        }
        (Annotation::OpaqueRedaction { bounds, .. }, HitPart::Body) => {
            (point.x - bounds.x, point.y - bounds.y)
        }
        (Annotation::NumberCallout { bubble, .. }, HitPart::Body) => {
            (point.x - bubble.x, point.y - bubble.y)
        }
        _ => (0.0, 0.0),
    }
}

pub(crate) fn direct_manipulation_hit(
    document: &ImageDocument,
    editor: &EditorState,
    point: ImagePoint,
    tolerance: f32,
) -> Option<Hit> {
    match editor.tool {
        Tool::Select => document.hit_test(point, tolerance),
        Tool::Redact => {
            let annotation = document.annotation(editor.selection?)?;
            matches!(annotation, Annotation::OpaqueRedaction { .. })
                .then(|| hit_test_annotation(annotation, point, tolerance))
                .flatten()
                .map(|part| Hit {
                    id: annotation.id(),
                    part,
                })
        }
        Tool::Number | Tool::Text => None,
    }
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
// Gesture handlers
// ---------------------------------------------------------------------------

pub(crate) fn handle_canvas_pressed(
    state: &mut super::ResultWorkspace,
    point: ImagePoint,
    now: std::time::Instant,
) -> Task<Message> {
    commit_text_draft(state);
    state.editor.copy_menu_open = false;

    let scale = current_scale(state);
    let tolerance = HIT_TOLERANCE_SCREEN / scale;

    let double_click = state.editor.last_press.is_some_and(|(at, p)| {
        now.duration_since(at).as_millis() <= DOUBLE_CLICK_WINDOW_MS
            && p.distance(point) <= DOUBLE_CLICK_SLOP_SCREEN / scale
    });
    state.editor.last_press = Some((now, point));

    match state.editor.tool {
        Tool::Select => {
            if double_click {
                if let Some(hit) = state.document.image.hit_test(point, tolerance) {
                    if let Some(Annotation::TextNote { position, text, .. }) =
                        state.document.image.annotation(hit.id).cloned().as_ref()
                    {
                        state.editor.drag = None;
                        state.editor.selection = Some(hit.id);
                        state.editor.text_draft = Some(TextDraft {
                            target: Some(hit.id),
                            position: *position,
                            content: iced::widget::text_editor::Content::with_text(text),
                        });
                        return iced::widget::operation::focus(state.text_editor_id.clone());
                    }
                }
            }
            match direct_manipulation_hit(&state.document.image, &state.editor, point, tolerance) {
                Some(hit) => {
                    let original = state
                        .document
                        .image
                        .annotation(hit.id)
                        .expect("hit returns existing annotations")
                        .clone();
                    state.editor.selection = Some(hit.id);
                    state.editor.drag = Some(DragState::EditAnnotation {
                        part: hit.part,
                        grab_offset: grab_offset(&original, hit.part, point),
                        current: original.clone(),
                        original,
                    });
                }
                None => {
                    state.editor.selection = None;
                    state.editor.drag = Some(DragState::Pan {
                        last_pointer: state.pointer_position,
                    });
                }
            }
            Task::none()
        }
        Tool::Number => {
            state.editor.drag = Some(DragState::CreateNumber {
                tip: point,
                bubble: point,
            });
            Task::none()
        }
        Tool::Text => {
            state.editor.text_draft = Some(TextDraft {
                target: None,
                position: point,
                content: iced::widget::text_editor::Content::new(),
            });
            iced::widget::operation::focus(state.text_editor_id.clone())
        }
        Tool::Redact => {
            if let Some(hit) =
                direct_manipulation_hit(&state.document.image, &state.editor, point, tolerance)
            {
                let original = state
                    .document
                    .image
                    .annotation(hit.id)
                    .expect("hit returns existing annotations")
                    .clone();
                state.editor.drag = Some(DragState::EditAnnotation {
                    part: hit.part,
                    grab_offset: grab_offset(&original, hit.part, point),
                    current: original.clone(),
                    original,
                });
                return Task::none();
            }
            state.editor.drag = Some(DragState::CreateRedaction {
                anchor: point,
                current: point,
            });
            Task::none()
        }
    }
}

pub(crate) fn handle_canvas_moved(
    state: &mut super::ResultWorkspace,
    point: ImagePoint,
) -> Task<Message> {
    let (w, h) = state.document.image.source().dimensions();
    let point = point.clamp_to(w, h);
    match &mut state.editor.drag {
        Some(DragState::CreateNumber { bubble, .. }) => {
            *bubble = point;
            Task::none()
        }
        Some(DragState::CreateRedaction { current, .. }) => {
            *current = point;
            Task::none()
        }
        Some(DragState::EditAnnotation {
            part,
            original,
            grab_offset,
            current,
        }) => {
            *current = dragged_annotation(original, *part, point, *grab_offset);
            Task::none()
        }
        Some(DragState::Pan { last_pointer }) => {
            let pointer = state.pointer_position;
            let delta = iced::Vector::new(pointer.x - last_pointer.x, pointer.y - last_pointer.y);
            *last_pointer = pointer;
            iced::widget::operation::scroll_by(
                state.scrollable_id.clone(),
                scrollable::AbsoluteOffset {
                    x: -delta.x,
                    y: -delta.y,
                },
            )
        }
        None => Task::none(),
    }
}

pub(crate) fn handle_canvas_released(
    state: &mut super::ResultWorkspace,
    point: ImagePoint,
) -> Task<Message> {
    let (w, h) = state.document.image.source().dimensions();
    let point = point.clamp_to(w, h);
    match state.editor.drag.take() {
        Some(DragState::CreateNumber { tip, .. }) => {
            let id = state.document.image.add_number_callout(tip, point);
            state.editor.selection = Some(id);
        }
        Some(DragState::CreateRedaction { anchor, .. }) => {
            if let Ok(id) = state
                .document
                .image
                .add_redaction(ImageRect::from_corners(anchor, point))
            {
                state.editor.selection = Some(id);
            }
        }
        Some(DragState::EditAnnotation {
            original, current, ..
        }) => {
            if current != original {
                let result = match &current {
                    Annotation::NumberCallout { tip, bubble, .. } => state
                        .document
                        .image
                        .set_number_points(original.id(), *tip, *bubble),
                    Annotation::TextNote { position, .. } => state
                        .document
                        .image
                        .set_text_position(original.id(), *position),
                    Annotation::OpaqueRedaction { bounds, .. } => state
                        .document
                        .image
                        .set_redaction_bounds(original.id(), *bounds),
                };
                if let Err(e) = result {
                    state.message = Some(InlineMessage::Error(e.to_string()));
                }
            }
        }
        Some(DragState::Pan { .. }) | None => {}
    }
    Task::none()
}

// ---------------------------------------------------------------------------
// Update
// ---------------------------------------------------------------------------

pub(crate) fn update(state: &mut super::ResultWorkspace, message: Message) -> Task<Message> {
    let task = update_inner(state, message);
    refresh_navigator(state);
    task
}

fn refresh_navigator(state: &mut super::ResultWorkspace) {
    let current = state.document.image.state_id();
    if state.editor.navigator_items_state != Some(current) {
        state.editor.navigator_items = state.document.image.navigator_items();
        state.editor.navigator_items_state = Some(current);
    }
}

fn update_inner(state: &mut super::ResultWorkspace, message: Message) -> Task<Message> {
    match message {
        Message::RequestClose => {
            commit_text_draft(state);
            state.pending_unredacted_action = None;
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
            if let super::workbench::WorkspaceMode::Workbench(ref wb) = state.mode {
                if super::workbench::state::has_pending_candidates(wb) {
                    state.message = Some(InlineMessage::Error(format!(
                        "{}\nApply them before safe copy/save.",
                        super::workbench::state::apply_skip_summary(wb)
                    )));
                    return Task::none();
                }
            }
            commit_text_draft(state);
            let safe_output = state.has_secure_redactions();
            let result = super::actions::copy_image(&copy_payload(state));
            Task::done(Message::CopyFinished {
                result,
                safe_output,
            })
        }
        Message::CopyOriginal => {
            state.editor.copy_menu_open = false;
            commit_text_draft(state);
            if super::secure_sharing::has_secure_redactions(&state.document) {
                state.pending_unredacted_action =
                    Some(super::secure_sharing::UnredactedAction::CopyOriginal);
                Task::none()
            } else {
                let result = super::actions::copy_image(&copy_original_payload(state));
                Task::done(Message::CopyFinished {
                    result,
                    safe_output: false,
                })
            }
        }
        Message::CopyFinished {
            result: Ok(()),
            safe_output,
        } => {
            let text = if safe_output {
                super::secure_sharing::COPY_SAFE_SUCCESS.to_string()
            } else {
                "Copied image".to_string()
            };
            state.message = Some(InlineMessage::success(text));
            Task::none()
        }
        Message::CopyFinished { result: Err(e), .. } => {
            state.message = Some(InlineMessage::Error(e));
            Task::none()
        }
        Message::SaveAs => {
            if let super::workbench::WorkspaceMode::Workbench(ref wb) = state.mode {
                if super::workbench::state::has_pending_candidates(wb) {
                    state.message = Some(InlineMessage::Error(format!(
                        "{}\nApply them before safe copy/save.",
                        super::workbench::state::apply_skip_summary(wb)
                    )));
                    return Task::none();
                }
            }
            commit_text_draft(state);
            let default_dir = crate::storage::Platform::current()
                .and_then(crate::storage::default_output_dir)
                .unwrap_or_else(|_| PathBuf::from("."));
            let default_name = super::secure_sharing::default_save_name(&state.document);
            Task::perform(
                super::actions::prompt_save_as(default_dir, default_name),
                Message::SavePathChosen,
            )
        }
        Message::SavePathChosen(Some(path)) => {
            let safe_output = state.has_secure_redactions();
            if super::secure_sharing::safe_export_overwrites_source(&state.document, &path) {
                state.message = Some(InlineMessage::Error(
                    super::secure_sharing::SAFE_EXPORT_OVERWRITE_ERROR.to_string(),
                ));
                return Task::none();
            }
            let image = save_payload(state);
            let saved_state_id = state.document.image.state_id();
            Task::perform(
                async move { super::actions::write_save_as(&image, &path) },
                move |result| Message::SaveFinished {
                    result,
                    saved_state_id,
                    safe_output,
                },
            )
        }
        Message::SavePathChosen(None) => Task::none(),
        Message::SaveFinished {
            result,
            saved_state_id,
            safe_output,
        } => {
            state.apply_save_as(result.map(Some), saved_state_id, safe_output);
            Task::none()
        }
        Message::Reveal => {
            commit_text_draft(state);
            match super::secure_sharing::reveal_action(&state.document) {
                super::secure_sharing::RevealAction::Disabled => Task::none(),
                super::secure_sharing::RevealAction::Immediate { path, .. } => {
                    Task::done(Message::RevealFinished(super::actions::reveal(path)))
                }
                super::secure_sharing::RevealAction::ConfirmUnredacted(_) => {
                    state.pending_unredacted_action =
                        Some(super::secure_sharing::UnredactedAction::RevealOriginal);
                    Task::none()
                }
            }
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
            commit_text_draft(state);
            state.viewport.zoom = mode;
            Task::none()
        }
        Message::ZoomStep(dir) => {
            commit_text_draft(state);
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
            if state.pending_unredacted_action.is_some() {
                state.pending_unredacted_action = None;
            } else if state.editor.copy_menu_open {
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
            commit_text_draft(state);
            state.editor.navigator_open = !state.editor.navigator_open;
            Task::none()
        }
        Message::NavigatorJump(id) => {
            commit_text_draft(state);
            if state.document.image.annotation(id).is_none() {
                state.editor.selection = None;
                return Task::none();
            }
            state.editor.selection = Some(id);
            if let Some(target) = state
                .editor
                .navigator_items
                .iter()
                .find(|i| i.id == id)
                .map(|i| i.center)
            {
                let geometry = geometry_for(
                    state.viewport.zoom,
                    state.original_size(),
                    state.viewport_bounds,
                );
                return iced::widget::operation::scroll_to(
                    state.scrollable_id.clone(),
                    super::navigator::jump_offset(target, &geometry, state.viewport_bounds),
                );
            }
            Task::none()
        }
        Message::ToggleCopyMenu => {
            commit_text_draft(state);
            state.editor.copy_menu_open = !state.editor.copy_menu_open;
            Task::none()
        }
        Message::TextDraftAction(action) => {
            if let Some(draft) = &mut state.editor.text_draft {
                draft.content.perform(action);
            }
            Task::none()
        }
        Message::CommitTextDraft => {
            commit_text_draft(state);
            Task::none()
        }
        Message::ConfirmUnredactedAction => match state.pending_unredacted_action.take() {
            Some(super::secure_sharing::UnredactedAction::CopyOriginal) => {
                let result = super::actions::copy_image(&copy_original_payload(state));
                Task::done(Message::CopyFinished {
                    result,
                    safe_output: false,
                })
            }
            Some(super::secure_sharing::UnredactedAction::RevealOriginal) => {
                let Some(path) = state.document.source_path.as_deref() else {
                    return Task::none();
                };
                Task::done(Message::RevealFinished(super::actions::reveal(path)))
            }
            None => Task::none(),
        },
        Message::CancelUnredactedAction => {
            state.pending_unredacted_action = None;
            Task::none()
        }
        Message::CanvasPressed(point) => {
            handle_canvas_pressed(state, point, std::time::Instant::now())
        }
        Message::CanvasMoved(point) => handle_canvas_moved(state, point),
        Message::CanvasReleased(point) => handle_canvas_released(state, point),
        Message::SmartRedaction => {
            state.mode = super::workbench::WorkspaceMode::Workbench(
                super::workbench::WorkbenchState::default(),
            );
            Task::none()
        }
        Message::Workbench(msg) => {
            let workbench = match &mut state.mode {
                super::workbench::WorkspaceMode::Workbench(wb) => wb,
                _ => return Task::none(),
            };
            match msg {
                super::workbench::WorkbenchMessage::RunEvent(event) => {
                    if let Some(entry) = super::workbench::state::event_to_activity_entry(&event) {
                        workbench.live_activity.push(entry);
                    }
                    Task::none()
                }
                super::workbench::WorkbenchMessage::RunTerminal(terminal) => {
                    workbench.live_activity.push(
                        super::workbench::state::ActivityEntry::TerminalLabel(
                            super::workbench::state::terminal_state_label(&terminal),
                        ),
                    );
                    if let Some(err) =
                        super::workbench::state::WorkbenchError::from_terminal(&terminal)
                    {
                        workbench.error = Some(err);
                    }
                    workbench.run_state = super::workbench::RunState::Terminal(terminal);
                    if let super::workbench::RunState::Terminal(
                        rollshot_agent::driver::RunTerminalState::ReadyForReview(ref ready),
                    ) = &workbench.run_state
                    {
                        workbench.pending_proposal = Some(ready.proposal.clone());
                        let ids: Vec<_> = ready.proposal.candidates.iter().map(|c| c.id).collect();
                        workbench.review = super::workbench::CandidateReview::from_candidates(&ids);
                        workbench.pending_draft = Some(super::workbench::PendingDraft {
                            source: ready.automation.source.clone(),
                            assistant_text: ready.assistant_text.clone(),
                            validation_summary: ready.automation.validation_summary.clone(),
                        });
                    }
                    Task::none()
                }
                super::workbench::WorkbenchMessage::RunFailed(e) => {
                    workbench.error = Some(e);
                    workbench.run_state =
                        super::workbench::RunState::Terminal(
                            rollshot_agent::driver::RunTerminalState::RuntimeFailure,
                        );
                    workbench.live_activity.push(
                        super::workbench::state::ActivityEntry::TerminalLabel(
                            super::workbench::state::terminal_state_label(
                                &rollshot_agent::driver::RunTerminalState::RuntimeFailure,
                            ),
                        ),
                    );
                    Task::none()
                }
                super::workbench::WorkbenchMessage::CancelRun => {
                    if let super::workbench::RunState::Running {
                        ref cancellation, ..
                    } = workbench.run_state
                    {
                        cancellation.cancel();
                    }
                    Task::none()
                }
                super::workbench::WorkbenchMessage::ApplyCandidates => {
                    if let Some(proposal) = workbench.pending_proposal.clone() {
                        match super::workbench::review::apply_candidates(
                            &proposal,
                            &workbench.review,
                            &mut state.document.image,
                        ) {
                            Ok(()) => {
                                workbench.pending_proposal = None;
                                workbench.review = super::workbench::CandidateReview::default();
                                workbench.selected_candidate = None;
                            }
                            Err(e) => workbench.error = Some(e),
                        }
                    }
                    Task::none()
                }
                super::workbench::WorkbenchMessage::CandidateSelected(id) => {
                    workbench.selected_candidate = Some(id);
                    Task::none()
                }
                super::workbench::WorkbenchMessage::CandidateDeselected => {
                    workbench.selected_candidate = None;
                    Task::none()
                }
                super::workbench::WorkbenchMessage::CandidateDeleted(id) => {
                    workbench.review.mark_rejected(id);
                    if workbench.selected_candidate == Some(id) {
                        workbench.selected_candidate = None;
                    }
                    Task::none()
                }
                super::workbench::WorkbenchMessage::CandidateUnrejected(id) => {
                    workbench.review.mark_pending(id);
                    Task::none()
                }
                super::workbench::WorkbenchMessage::CandidateMoved { id, new_bounds } => {
                    workbench.review.mark_modified(
                        id,
                        rollshot_edit_proposal::ProposedEdit::AddRedaction { bounds: new_bounds },
                    );
                    Task::none()
                }
                super::workbench::WorkbenchMessage::AddManualCandidate { bounds } => {
                    let id =
                        rollshot_edit_proposal::CandidateId(workbench.next_manual_candidate_id);
                    workbench.next_manual_candidate_id += 1;
                    if let Some(proposal) = &mut workbench.pending_proposal {
                        use rollshot_edit_proposal::{
                            ProposedCandidate, ProposedEdit, Provenance, ProvenanceSource,
                        };
                        proposal.candidates.push(ProposedCandidate {
                            id,
                            edit: ProposedEdit::AddRedaction { bounds },
                            confidence: 1.0,
                            label: "manual".into(),
                            rationale: Some("Manually added missing candidate".into()),
                            provenance: Provenance {
                                source: ProvenanceSource::Manual,
                            },
                        });
                    }
                    workbench.review.mark_modified(
                        id,
                        rollshot_edit_proposal::ProposedEdit::AddRedaction { bounds },
                    );
                    Task::none()
                }
                super::workbench::WorkbenchMessage::NextWarning => Task::none(),
                super::workbench::WorkbenchMessage::JumpToCandidate(_id) => Task::none(),
                super::workbench::WorkbenchMessage::ComposerChanged(s) => {
                    workbench.composer = s;
                    Task::none()
                }
                super::workbench::WorkbenchMessage::SendRequested => {
                    let user_message = std::mem::take(&mut workbench.composer);
                    if user_message.is_empty() {
                        return Task::none();
                    }
                    let (w, h) = state.document.image.source().dimensions();
                    let params = super::workbench::PendingRunParams {
                        user_message,
                        image_dims: (w, h),
                        active_revision_source: workbench
                            .active_revision
                            .as_ref()
                            .map(|r| r.artifact.source.clone()),
                        mode: super::workbench::RunKind::Author,
                    };
                    workbench.disclosure_pending = true;
                    workbench.pending_run = Some(params);
                    Task::none()
                }
                super::workbench::WorkbenchMessage::PayloadModeSelected(m) => {
                    workbench.payload_mode = m;
                    Task::none()
                }
                super::workbench::WorkbenchMessage::DisclosureConfirmed => {
                    workbench.disclosure_pending = false;
                    let Some(params) = workbench.pending_run.take() else {
                        return Task::none();
                    };
                    let image = state.document.image.source().clone();
                    let session_id = workbench.session.session_id;
                    let session = std::mem::replace(
                        &mut workbench.session,
                        rollshot_agent::domain::AgentSession::new(session_id),
                    );
                    match super::workbench::run::start_agent_run(
                        &params,
                        &image,
                        &workbench.provider_config,
                        &workbench.budget,
                        session,
                        workbench.payload_mode,
                    ) {
                        Ok((task, cancellation)) => {
                            workbench.run_state =
                                super::workbench::RunState::Running { cancellation };
                            task
                        }
                        Err(e) => {
                            workbench.error = Some(e);
                            Task::none()
                        }
                    }
                }
                super::workbench::WorkbenchMessage::DisclosureCancelled => {
                    workbench.disclosure_pending = false;
                    workbench.pending_run = None;
                    Task::none()
                }
                super::workbench::WorkbenchMessage::SavePresetOrRevision => {
                    if let Some(draft) = workbench.pending_draft.clone() {
                        if let Ok(config_dir) = crate::daemon::config::rollshot_config_dir() {
                            let store =
                                rollshot_preset::PresetStore::open(config_dir.join("presets"));
                            let preset_id = rollshot_preset::PresetId("workbench-draft".into());
                            if store.load_preset(&preset_id).is_err() {
                                let _ = store.create_preset(
                                    preset_id.clone(),
                                    "Workbench Draft".into(),
                                    "Authored via Smart Redaction".into(),
                                    chrono::Utc::now().to_rfc3339(),
                                );
                            }
                            match super::workbench::review::save_revision(
                                &store,
                                &preset_id,
                                &draft.source,
                                None,
                                workbench.session.session_id.get(),
                                chrono::Utc::now().to_rfc3339(),
                            ) {
                                Ok(()) => workbench.pending_draft = None,
                                Err(e) => workbench.error = Some(e),
                            }
                        } else {
                            workbench.error = Some(super::workbench::state::WorkbenchError::Config);
                        }
                    }
                    Task::none()
                }
                super::workbench::WorkbenchMessage::ImStart => {
                    if workbench.pending_proposal.is_some() && !workbench.review.is_empty() {
                        workbench.disclosure_pending = true;
                    }
                    Task::none()
                }
                super::workbench::WorkbenchMessage::AskAgentToRevise
                | super::workbench::WorkbenchMessage::DiscardDraft
                | super::workbench::WorkbenchMessage::DiscardCandidates
                | super::workbench::WorkbenchMessage::ToggleAdvancedDetails
                | super::workbench::WorkbenchMessage::OpenProviderSettings
                | super::workbench::WorkbenchMessage::DisclosureRequested(_) => Task::none(),
            }
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
fn commit_text_draft(state: &mut super::ResultWorkspace) {
    let Some(draft) = state.editor.text_draft.take() else {
        return;
    };
    let text = draft.content.text().trim_end().to_string();
    match draft.target {
        None => {
            if let Ok(id) = state.document.image.add_text_note(draft.position, text) {
                state.editor.selection = Some(id);
            }
        }
        Some(id) => {
            let _ = state.document.image.set_text(id, text);
        }
    }
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
// Keyboard routing
// ---------------------------------------------------------------------------

pub(crate) fn map_key_press(
    key: &keyboard::Key,
    modifiers: keyboard::Modifiers,
    captured: bool,
) -> Option<Message> {
    use keyboard::key::Named;
    if matches!(key, keyboard::Key::Named(Named::Escape)) {
        return Some(Message::EscapePressed);
    }
    if captured {
        return None;
    }
    let command = zoom_modifier_held(modifiers);
    match key {
        keyboard::Key::Named(Named::Delete) | keyboard::Key::Named(Named::Backspace) => {
            Some(Message::DeleteSelected)
        }
        keyboard::Key::Character(c) if command => match c.as_str() {
            "z" if modifiers.shift() => Some(Message::Redo),
            "z" => Some(Message::Undo),
            "c" => Some(Message::Copy),
            _ => None,
        },
        keyboard::Key::Character(c) if !modifiers.alt() => match c.as_str() {
            "v" => Some(Message::SelectTool(Tool::Select)),
            "n" => Some(Message::SelectTool(Tool::Number)),
            "t" => Some(Message::SelectTool(Tool::Text)),
            "r" => Some(Message::SelectTool(Tool::Redact)),
            _ => None,
        },
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Subscription
// ---------------------------------------------------------------------------

pub(crate) fn subscription(state: &super::ResultWorkspace) -> Subscription<Message> {
    let mut subs = vec![
        iced::window::close_requests().map(|_id| Message::RequestClose),
        iced::event::listen_with(|event, status, _window| match event {
            iced::Event::Keyboard(keyboard::Event::ModifiersChanged(m)) => {
                Some(Message::ModifiersChanged(m))
            }
            iced::Event::Keyboard(keyboard::Event::KeyPressed { key, modifiers, .. }) => {
                map_key_press(&key, modifiers, status == iced::event::Status::Captured)
            }
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
    use rollshot_image_document::{Annotation, ImagePoint, ImageRect};
    use std::path::Path;
    use std::time::{Duration, Instant};

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

    #[test]
    fn save_completion_marks_the_written_state_not_newer_edits() {
        let mut state = unsaved_workspace();
        state
            .document
            .image
            .add_number_callout(ImagePoint::new(0.5, 0.5), ImagePoint::new(0.5, 0.5));
        let written_state_id = state.document.image.state_id();
        state
            .document
            .image
            .add_number_callout(ImagePoint::new(1.0, 1.0), ImagePoint::new(1.0, 1.0));

        let _ = update(
            &mut state,
            Message::SaveFinished {
                result: Ok(PathBuf::from("/tmp/annotated.png")),
                saved_state_id: written_state_id,
                safe_output: false,
            },
        );

        assert_eq!(state.editor.saved_state_id, written_state_id);
        assert!(
            state.annotations_dirty(),
            "edits made after the save payload was captured remain dirty"
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

    // -- gesture tests (Task 19) ----------------------------------------------

    fn workspace_with_size(w: u32, h: u32) -> super::super::ResultWorkspace {
        let img = RgbaImage::from_pixel(w, h, Rgba([100, 150, 200, 255]));
        let mut ws = super::super::ResultWorkspace::new(
            super::super::document::ResultDocument::unsaved(img),
            None,
        );
        ws.viewport.zoom = ZoomMode::ActualSize;
        ws.apply_viewport_bounds(Size::new(w as f32, h as f32));
        ws
    }

    fn press_move_release(
        state: &mut super::super::ResultWorkspace,
        from: ImagePoint,
        to: ImagePoint,
    ) {
        let _ = update(state, Message::CanvasPressed(from));
        let _ = update(state, Message::CanvasMoved(to));
        let _ = update(state, Message::CanvasReleased(to));
    }

    #[test]
    fn number_click_creates_coincident_stamp_and_keeps_tool_active() {
        let mut state = workspace_with_size(200, 200);
        let _ = update(&mut state, Message::SelectTool(Tool::Number));
        let p = ImagePoint::new(1.0, 1.0);
        press_move_release(&mut state, p, p);
        match &state.document.image.annotations()[0] {
            Annotation::NumberCallout {
                tip,
                bubble,
                number,
                ..
            } => {
                assert_eq!(tip, bubble, "click → coincident stamp");
                assert_eq!(*number, 1);
            }
            _ => panic!(),
        }
        assert_eq!(
            state.editor.tool,
            Tool::Number,
            "spec §9.2: tool stays active"
        );
        press_move_release(
            &mut state,
            ImagePoint::new(1.5, 1.5),
            ImagePoint::new(1.5, 1.5),
        );
        assert_eq!(state.document.image.next_number(), 3);
    }

    #[test]
    fn number_drag_anchors_tip_and_separates_bubble_in_one_edit() {
        let mut state = workspace_with_size(200, 200);
        let _ = update(&mut state, Message::SelectTool(Tool::Number));
        press_move_release(
            &mut state,
            ImagePoint::new(0.5, 0.5),
            ImagePoint::new(1.8, 1.8),
        );
        assert_eq!(state.document.image.annotations().len(), 1);
        match &state.document.image.annotations()[0] {
            Annotation::NumberCallout { tip, bubble, .. } => {
                assert_eq!(*tip, ImagePoint::new(0.5, 0.5), "tip anchored at press");
                assert_eq!(*bubble, ImagePoint::new(1.8, 1.8), "bubble follows drag");
            }
            _ => panic!(),
        }
        let mut undo_steps = 0;
        while state.document.image.undo() {
            undo_steps += 1;
        }
        assert_eq!(undo_steps, 1, "spec §5.2: one drag = one history entry");
    }

    #[test]
    fn redaction_drag_creates_rect_and_zero_drag_creates_nothing() {
        let mut state = workspace_with_size(200, 200);
        let _ = update(&mut state, Message::SelectTool(Tool::Redact));
        press_move_release(
            &mut state,
            ImagePoint::new(0.0, 0.0),
            ImagePoint::new(2.0, 2.0),
        );
        assert_eq!(state.document.image.annotations().len(), 1);
        press_move_release(
            &mut state,
            ImagePoint::new(1.0, 1.0),
            ImagePoint::new(1.0, 1.0),
        );
        assert_eq!(state.document.image.annotations().len(), 1);
    }

    #[test]
    fn select_click_on_annotation_selects_without_history_entry() {
        let mut state = workspace_with_size(200, 200);
        let id = state
            .document
            .image
            .add_number_callout(ImagePoint::new(1.0, 1.0), ImagePoint::new(1.0, 1.0));
        let s = state.document.image.state_id();
        let _ = update(&mut state, Message::SelectTool(Tool::Select));
        press_move_release(
            &mut state,
            ImagePoint::new(1.0, 1.0),
            ImagePoint::new(1.0, 1.0),
        );
        assert_eq!(state.editor.selection, Some(id));
        assert_eq!(
            state.document.image.state_id(),
            s,
            "no-move release edits nothing"
        );
    }

    #[test]
    fn select_click_on_empty_canvas_clears_selection_without_edits() {
        let mut state = workspace_with_size(100, 100);
        let id = state
            .document
            .image
            .add_number_callout(ImagePoint::new(10.0, 10.0), ImagePoint::new(10.0, 10.0));
        state.editor.selection = Some(id);
        let s = state.document.image.state_id();
        press_move_release(
            &mut state,
            ImagePoint::new(90.0, 90.0),
            ImagePoint::new(90.0, 90.0),
        );
        assert_eq!(state.editor.selection, None);
        assert_eq!(state.document.image.state_id(), s);
    }

    #[test]
    fn dragging_the_bubble_commits_one_set_points_edit() {
        let mut state = workspace_with_size(200, 200);
        let id = state
            .document
            .image
            .add_number_callout(ImagePoint::new(20.0, 20.0), ImagePoint::new(100.0, 100.0));
        press_move_release(
            &mut state,
            ImagePoint::new(100.0, 100.0),
            ImagePoint::new(150.0, 150.0),
        );
        match state.document.image.annotation(id).unwrap() {
            Annotation::NumberCallout { tip, bubble, .. } => {
                assert_eq!(*bubble, ImagePoint::new(150.0, 150.0));
                assert_eq!(*tip, ImagePoint::new(20.0, 20.0), "tip moves independently");
            }
            _ => panic!(),
        }
    }

    #[test]
    fn resizing_a_redaction_commits_new_bounds() {
        let mut state = workspace_with_size(200, 200);
        let id = state
            .document
            .image
            .add_redaction(ImageRect {
                x: 50.0,
                y: 50.0,
                width: 40.0,
                height: 30.0,
            })
            .unwrap();
        press_move_release(
            &mut state,
            ImagePoint::new(90.0, 80.0),
            ImagePoint::new(120.0, 110.0),
        );
        match state.document.image.annotation(id).unwrap() {
            Annotation::OpaqueRedaction { bounds, .. } => {
                assert_eq!(
                    *bounds,
                    ImageRect {
                        x: 50.0,
                        y: 50.0,
                        width: 70.0,
                        height: 60.0
                    }
                );
            }
            _ => panic!(),
        }
    }

    #[test]
    fn redact_tool_resizes_the_selected_redaction_from_a_handle() {
        let mut state = workspace_with_size(200, 200);
        let id = state
            .document
            .image
            .add_redaction(ImageRect {
                x: 50.0,
                y: 50.0,
                width: 40.0,
                height: 30.0,
            })
            .unwrap();
        state.editor.selection = Some(id);
        let _ = update(&mut state, Message::SelectTool(Tool::Redact));

        press_move_release(
            &mut state,
            ImagePoint::new(90.0, 80.0),
            ImagePoint::new(120.0, 110.0),
        );

        assert_eq!(state.document.image.annotations().len(), 1);
        match state.document.image.annotation(id).unwrap() {
            Annotation::OpaqueRedaction { bounds, .. } => {
                assert_eq!(
                    *bounds,
                    ImageRect {
                        x: 50.0,
                        y: 50.0,
                        width: 70.0,
                        height: 60.0
                    }
                );
            }
            _ => panic!(),
        }
    }

    #[test]
    fn redact_tool_moves_the_selected_redaction_from_its_body() {
        let mut state = workspace_with_size(200, 200);
        let id = state
            .document
            .image
            .add_redaction(ImageRect {
                x: 50.0,
                y: 50.0,
                width: 40.0,
                height: 30.0,
            })
            .unwrap();
        state.editor.selection = Some(id);
        let _ = update(&mut state, Message::SelectTool(Tool::Redact));

        press_move_release(
            &mut state,
            ImagePoint::new(70.0, 65.0),
            ImagePoint::new(100.0, 95.0),
        );

        assert_eq!(state.document.image.annotations().len(), 1);
        match state.document.image.annotation(id).unwrap() {
            Annotation::OpaqueRedaction { bounds, .. } => {
                assert_eq!(
                    *bounds,
                    ImageRect {
                        x: 80.0,
                        y: 80.0,
                        width: 40.0,
                        height: 30.0
                    }
                );
            }
            _ => panic!(),
        }
    }

    #[test]
    fn redact_tool_still_creates_on_empty_canvas_with_a_selection() {
        let mut state = workspace_with_size(200, 200);
        let id = state
            .document
            .image
            .add_redaction(ImageRect {
                x: 50.0,
                y: 50.0,
                width: 40.0,
                height: 30.0,
            })
            .unwrap();
        state.editor.selection = Some(id);
        let _ = update(&mut state, Message::SelectTool(Tool::Redact));

        press_move_release(
            &mut state,
            ImagePoint::new(120.0, 120.0),
            ImagePoint::new(160.0, 160.0),
        );

        assert_eq!(state.document.image.annotations().len(), 2);
    }

    // -- text editor tests (Task 20) ------------------------------------------

    fn type_text(state: &mut super::super::ResultWorkspace, s: &str) {
        if let Some(draft) = &mut state.editor.text_draft {
            for ch in s.chars() {
                draft
                    .content
                    .perform(iced::widget::text_editor::Action::Edit(
                        iced::widget::text_editor::Edit::Insert(ch),
                    ));
            }
        }
    }

    #[test]
    fn typing_then_commit_creates_exactly_one_edit() {
        let mut state = workspace_with_size(200, 100);
        let _ = update(&mut state, Message::SelectTool(Tool::Text));
        let _ = update(
            &mut state,
            Message::CanvasPressed(ImagePoint::new(10.0, 10.0)),
        );
        assert!(state.editor.text_draft.is_some());
        type_text(&mut state, "hello");
        let _ = update(&mut state, Message::CommitTextDraft);
        assert!(state.editor.text_draft.is_none());
        match &state.document.image.annotations()[0] {
            Annotation::TextNote { text, .. } => {
                assert_eq!(text, "hello")
            }
            _ => panic!(),
        }
        let mut undo_steps = 0;
        while state.document.image.undo() {
            undo_steps += 1;
        }
        assert_eq!(undo_steps, 1, "spec §9.3: whole text = one undo entry");
    }

    #[test]
    fn empty_draft_commit_creates_nothing_and_esc_cancels() {
        let mut state = workspace_with_size(200, 100);
        let _ = update(&mut state, Message::SelectTool(Tool::Text));
        let _ = update(
            &mut state,
            Message::CanvasPressed(ImagePoint::new(10.0, 10.0)),
        );
        let _ = update(&mut state, Message::CommitTextDraft);
        assert!(state.document.image.annotations().is_empty(), "spec §15");

        let _ = update(
            &mut state,
            Message::CanvasPressed(ImagePoint::new(10.0, 10.0)),
        );
        type_text(&mut state, "draft");
        let _ = update(&mut state, Message::EscapePressed);
        assert!(state.editor.text_draft.is_none(), "esc cancels the draft");
        assert!(state.document.image.annotations().is_empty());
    }

    #[test]
    fn clicking_outside_commits_the_open_draft() {
        let mut state = workspace_with_size(200, 100);
        let _ = update(&mut state, Message::SelectTool(Tool::Text));
        let _ = update(
            &mut state,
            Message::CanvasPressed(ImagePoint::new(10.0, 10.0)),
        );
        type_text(&mut state, "note");
        let _ = update(
            &mut state,
            Message::CanvasPressed(ImagePoint::new(100.0, 50.0)),
        );
        assert_eq!(state.document.image.annotations().len(), 1);
    }

    #[test]
    fn clicking_non_canvas_controls_commits_the_open_draft() {
        for message in [
            Message::SetZoom(ZoomMode::ActualSize),
            Message::ZoomStep(ZoomDirection::In),
            Message::ToggleNavigator,
            Message::ToggleCopyMenu,
            Message::Reveal,
        ] {
            let mut state = workspace_with_size(200, 100);
            let _ = update(&mut state, Message::SelectTool(Tool::Text));
            let _ = update(
                &mut state,
                Message::CanvasPressed(ImagePoint::new(10.0, 10.0)),
            );
            type_text(&mut state, "note");

            let _ = update(&mut state, message);

            assert!(state.editor.text_draft.is_none());
            assert_eq!(state.document.image.annotations().len(), 1);
        }
    }

    #[test]
    fn double_click_reedit_commits_one_changed_text_edit() {
        let mut state = workspace_with_size(300, 100);
        let id = state
            .document
            .image
            .add_text_note(ImagePoint::new(10.0, 10.0), "old".to_string())
            .unwrap();
        let _ = update(&mut state, Message::SelectTool(Tool::Select));
        let now = Instant::now();
        let _ = handle_canvas_pressed(&mut state, ImagePoint::new(15.0, 15.0), now);
        let _ = handle_canvas_pressed(
            &mut state,
            ImagePoint::new(15.0, 15.0),
            now + Duration::from_millis(100),
        );
        let draft = state
            .editor
            .text_draft
            .as_ref()
            .expect("re-edit draft open");
        assert_eq!(draft.target, Some(id));
        assert_eq!(draft.content.text().trim_end(), "old");
        state.editor.text_draft.as_mut().unwrap().content =
            iced::widget::text_editor::Content::with_text("new");
        let _ = update(&mut state, Message::CommitTextDraft);
        match state.document.image.annotation(id).unwrap() {
            Annotation::TextNote { text, .. } => assert_eq!(text, "new"),
            _ => panic!(),
        }
    }

    // -- navigator jump (Task 21) -------------------------------------------

    #[test]
    fn navigator_jump_selects_and_ignores_stale_ids() {
        let mut state = workspace_with_size(100, 100);
        let id = state
            .document
            .image
            .add_number_callout(ImagePoint::new(10.0, 10.0), ImagePoint::new(10.0, 10.0));
        let _ = update(&mut state, Message::NavigatorJump(id));
        assert_eq!(state.editor.selection, Some(id));
        let _ = update(&mut state, Message::Undo);
        let _ = update(&mut state, Message::NavigatorJump(id));
        assert_eq!(state.editor.selection, None);
    }

    // -- safe copy/save routing (Task 2) ------------------------------------

    use super::super::secure_sharing::{
        UnredactedAction, COPY_SAFE_SUCCESS, SAFE_EXPORT_OVERWRITE_ERROR, SAVE_SAFE_SUCCESS,
    };

    #[test]
    fn safe_copy_completion_uses_safe_message() {
        let mut state = saved_workspace();
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

        let _ = update(
            &mut state,
            Message::CopyFinished {
                result: Ok(()),
                safe_output: true,
            },
        );
        assert_eq!(state.message_text().as_deref(), Some(COPY_SAFE_SUCCESS));
    }

    #[test]
    fn safe_save_rejects_source_before_write_and_preserves_state() {
        let mut state = saved_workspace();
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
        let state_id = state.document.image.state_id();

        let _ = update(
            &mut state,
            Message::SavePathChosen(Some(PathBuf::from("/tmp/result.png"))),
        );

        assert_eq!(
            state.message_text().as_deref(),
            Some(SAFE_EXPORT_OVERWRITE_ERROR)
        );
        assert_eq!(state.document.image.state_id(), state_id);
        assert!(state.document.last_export_path.is_none());
        assert!(state.annotations_dirty());
    }

    #[test]
    fn safe_save_completion_records_safe_message_and_path() {
        let mut state = saved_workspace();
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
        let state_id = state.document.image.state_id();

        state.apply_save_as(Ok(Some(PathBuf::from("/tmp/safe.png"))), state_id, true);

        assert_eq!(state.message_text().as_deref(), Some(SAVE_SAFE_SUCCESS));
        assert_eq!(
            state.document.last_export_path.as_deref(),
            Some(Path::new("/tmp/safe.png"))
        );
        assert!(state.document.last_export_is_safe);
        assert!(!state.annotations_dirty());
    }

    #[test]
    fn unredacted_save_is_not_revealed_as_safe_after_adding_redaction() {
        let mut state = saved_workspace();
        let state_id = state.document.image.state_id();
        state.apply_save_as(
            Ok(Some(PathBuf::from("/tmp/unredacted-export.png"))),
            state_id,
            false,
        );
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

        assert!(!state.document.last_export_is_safe);
        assert_eq!(
            super::super::secure_sharing::reveal_action(&state.document),
            super::super::secure_sharing::RevealAction::ConfirmUnredacted(Path::new(
                "/tmp/result.png"
            ))
        );
    }

    // -- keyboard routing (Task 22) -----------------------------------------

    fn zmod() -> keyboard::Modifiers {
        #[cfg(target_os = "macos")]
        {
            keyboard::Modifiers::COMMAND
        }
        #[cfg(not(target_os = "macos"))]
        {
            keyboard::Modifiers::CTRL
        }
    }

    #[test]
    fn key_mapping_routes_tools_undo_redo_delete_copy() {
        use keyboard::{key::Named, Key};
        let none = keyboard::Modifiers::default();
        assert!(matches!(
            map_key_press(&Key::Character("n".into()), none, false),
            Some(Message::SelectTool(Tool::Number))
        ));
        assert!(matches!(
            map_key_press(&Key::Character("z".into()), zmod(), false),
            Some(Message::Undo)
        ));
        assert!(matches!(
            map_key_press(
                &Key::Character("z".into()),
                zmod() | keyboard::Modifiers::SHIFT,
                false
            ),
            Some(Message::Redo)
        ));
        assert!(matches!(
            map_key_press(&Key::Named(Named::Delete), none, false),
            Some(Message::DeleteSelected)
        ));
        assert!(matches!(
            map_key_press(&Key::Character("c".into()), zmod(), false),
            Some(Message::Copy)
        ));
    }

    #[test]
    fn captured_keys_are_ignored_except_escape() {
        use keyboard::{key::Named, Key};
        let none = keyboard::Modifiers::default();
        assert!(map_key_press(&Key::Character("n".into()), none, true).is_none());
        assert!(map_key_press(&Key::Named(Named::Backspace), none, true).is_none());
        assert!(matches!(
            map_key_press(&Key::Named(Named::Escape), none, true),
            Some(Message::EscapePressed)
        ));
    }

    #[test]
    fn plain_characters_do_not_fire_with_command_modifiers_held() {
        use keyboard::Key;
        assert!(map_key_press(&Key::Character("n".into()), zmod(), false).is_none());
    }

    // -- unredacted-action confirmation (Task 3) ----------------------------

    #[test]
    fn redacted_copy_original_requires_fresh_confirmation() {
        let mut state = saved_workspace();
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

        let _ = update(&mut state, Message::CopyOriginal);
        assert_eq!(
            state.pending_unredacted_action,
            Some(UnredactedAction::CopyOriginal)
        );
        let _ = update(&mut state, Message::CancelUnredactedAction);
        assert_eq!(state.pending_unredacted_action, None);
        let _ = update(&mut state, Message::CopyOriginal);
        assert_eq!(
            state.pending_unredacted_action,
            Some(UnredactedAction::CopyOriginal)
        );
        let _ = update(&mut state, Message::ConfirmUnredactedAction);
        assert_eq!(state.pending_unredacted_action, None);
        let _ = update(&mut state, Message::CopyOriginal);
        assert_eq!(
            state.pending_unredacted_action,
            Some(UnredactedAction::CopyOriginal)
        );
    }

    #[test]
    fn redacted_reveal_original_requires_confirmation() {
        let mut state = saved_workspace();
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

        let _ = update(&mut state, Message::Reveal);
        assert_eq!(
            state.pending_unredacted_action,
            Some(UnredactedAction::RevealOriginal)
        );
    }

    #[test]
    fn request_close_clears_unredacted_confirmation_before_close_routing() {
        let mut state = saved_workspace();
        state.pending_unredacted_action = Some(UnredactedAction::CopyOriginal);
        let _ = update(&mut state, Message::RequestClose);
        assert_eq!(state.pending_unredacted_action, None);
    }

    #[test]
    fn escape_cancels_pending_unredacted_confirmation_without_closing() {
        let mut state = saved_workspace();
        state.pending_unredacted_action = Some(UnredactedAction::RevealOriginal);
        let _ = update(&mut state, Message::EscapePressed);
        assert_eq!(state.pending_unredacted_action, None);
        // Esc cancelled the blocking dialog; it must not have escalated to close.
        assert!(state.pending_discard.is_none());
    }
}
