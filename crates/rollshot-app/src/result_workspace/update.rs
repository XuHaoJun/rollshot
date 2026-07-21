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
use super::pixelate_preview::{generate_preview, requested_pixelate_keys};
use super::two_point::{bounded_constrained_endpoint, gesture_meets_threshold};
use super::{CloseDecision, InlineMessage, WHEEL_LINE_PX};
use rollshot_image_document::{
    hit_test_annotation, Annotation, AnnotationId, Hit, HitPart, ImageDocument, ImagePoint,
    ImageRect, Rgb8, StrokeStyle, TwoPointKind,
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
    /// User pressed the platform copy shortcut.
    KeyboardCopy,
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
    /// Toggle the More overflow menu dropdown.
    ToggleMoreMenu,
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
    /// Export a bug-report Issue Pack from the result workspace.
    ExportBugReport,
    /// Toggle the review-confirmed checkbox in the Issue Pack dialog.
    IssuePackReviewChanged(bool),
    /// Switch to the redaction tool from the Issue Pack dialog.
    IssuePackReviewRedactions,
    /// Begin exporting an Issue Pack to a folder.
    IssuePackExportFolder,
    /// Begin exporting an Issue Pack to a ZIP file.
    IssuePackExportZip,
    /// The async folder-picker returned (None = cancelled).
    IssuePackFolderChosen(Option<PathBuf>),
    /// Background Issue Pack export completed.
    #[allow(private_interfaces)]
    IssuePackFinished(Result<crate::issue_pack::IssuePackExportResult, String>),
    /// Close the Issue Pack dialog without exporting.
    IssuePackCancel,
    /// Messages forwarded from the workbench sub-state.
    #[allow(dead_code)] // SP6 scaffolding: constructed by later tasks
    Workbench(super::workbench::WorkbenchMessage),
    // -- property editing (Task 4) -------------------------------------------
    /// Set the next number value for the number tool default.
    NextNumberInputChanged(String),
    /// Commit the next number input to the document.
    CommitNextNumber,
    /// Open the color picker for a specific property.
    OpenColorPicker(super::properties::ColorProperty),
    /// Live-preview a color during color picker interaction (no doc mutation).
    PreviewColor(rollshot_image_document::Rgb8),
    /// Update the hex input field in the color picker.
    ColorHexChanged(String),
    /// Commit the color transaction to the document (annotation or default).
    ApplyColor,
    /// Cancel the color transaction without mutation.
    CancelColor,
    /// Live-preview a stroke width during slider interaction (no doc mutation).
    PreviewStrokeWidth(f32),
    /// Commit the stroke-width transaction to the document or active-tool default.
    ApplyStrokeWidth,
    /// Cancel the stroke-width transaction without mutation.
    #[allow(dead_code)]
    CancelStrokeWidth,
    /// Live-preview a stroke opacity during slider interaction (no doc mutation).
    PreviewStrokeOpacity(f32),
    /// Commit the opacity transaction to the document or active-tool default.
    ApplyStrokeOpacity,
    /// Cancel the opacity transaction without mutation.
    #[allow(dead_code)]
    CancelStrokeOpacity,
    /// Set the number size for number callouts.
    SetNumberSize(rollshot_image_document::NumberSize),
    /// Set the text size for text notes.
    SetTextSize(rollshot_image_document::TextSize),
    /// Toggle the text background on/off for text notes.
    ToggleTextBackground,
    /// Live-preview a shape stroke width during slider interaction.
    PreviewShapeStrokeWidth(f32),
    /// Toggle fill on/off for the active shape tool or selected shape.
    ToggleShapeFill,
    /// Commit the shape style transaction to the document.
    ApplyShapeStyle,
    /// Cancel the shape style transaction without mutation.
    #[allow(dead_code)]
    CancelShapeStyle,
    /// Live-preview a pixelate block size during slider interaction.
    PreviewPixelateBlockSize(u32),
    /// Commit the pixelate block size transaction.
    ApplyPixelateBlockSize,
    /// Cancel the pixelate block size transaction without mutation.
    #[allow(dead_code)]
    CancelPixelateBlockSize,
    /// Toggle the shapes selector menu.
    ToggleShapesMenu,
    /// Select a specific shape kind from the selector.
    SelectShape(rollshot_image_document::ShapeKind),
    /// Activate the remembered shape tool (primary button / `S` shortcut).
    SelectRememberedShape,
    /// A pixelate preview generation completed (async worker).
    PixelatePreviewReady(
        super::pixelate_preview::PreviewRequest,
        Result<
            super::pixelate_preview::PreviewPixels,
            super::pixelate_preview::PreviewGenerationError,
        >,
    ),
    #[cfg(feature = "ocr")]
    OcrPrepared(Result<Vec<super::ocr_text::OcrTextItem>, super::ocr_text::ProductOcrError>),
    #[cfg(feature = "ocr")]
    #[allow(dead_code)]
    OcrSelectionStarted(super::ocr_text::TextCursor),
    #[cfg(feature = "ocr")]
    #[allow(dead_code)]
    OcrSelectionChanged(super::ocr_text::TextCursor),
    #[cfg(feature = "ocr")]
    #[allow(dead_code)]
    OcrSelectionFinished(super::ocr_text::TextCursor),
    #[cfg(feature = "ocr")]
    SelectAllOcrText,
    #[cfg(feature = "ocr")]
    CopyOcrSelection,
    #[cfg(feature = "ocr")]
    #[allow(dead_code)]
    CopyAllOcrText,
    #[cfg(feature = "ocr")]
    CopyOcrFinished(Result<(), String>),
}

impl PartialEq for Message {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::RequestClose, Self::RequestClose) => true,
            (Self::ConfirmDiscard, Self::ConfirmDiscard) => true,
            (Self::KeepUnsaved, Self::KeepUnsaved) => true,
            (Self::DismissMessage, Self::DismissMessage) => true,
            (Self::Copy, Self::Copy) => true,
            (Self::KeyboardCopy, Self::KeyboardCopy) => true,
            (Self::CopyOriginal, Self::CopyOriginal) => true,
            (
                Self::CopyFinished {
                    result: a_result,
                    safe_output: a_safe,
                },
                Self::CopyFinished {
                    result: b_result,
                    safe_output: b_safe,
                },
            ) => a_result == b_result && a_safe == b_safe,
            (Self::SaveAs, Self::SaveAs) => true,
            (Self::SavePathChosen(a), Self::SavePathChosen(b)) => a == b,
            (
                Self::SaveFinished {
                    result: a_result,
                    saved_state_id: a_id,
                    safe_output: a_safe,
                },
                Self::SaveFinished {
                    result: b_result,
                    saved_state_id: b_id,
                    safe_output: b_safe,
                },
            ) => a_result == b_result && a_id == b_id && a_safe == b_safe,
            (Self::Reveal, Self::Reveal) => true,
            (Self::RevealFinished(a), Self::RevealFinished(b)) => a == b,
            (Self::Tick(a), Self::Tick(b)) => *a == *b,
            (Self::SetZoom(a), Self::SetZoom(b)) => a == b,
            (Self::ZoomStep(a), Self::ZoomStep(b)) => a == b,
            (
                Self::ViewportChanged {
                    bounds: a_bounds,
                    offset: a_offset,
                },
                Self::ViewportChanged {
                    bounds: b_bounds,
                    offset: b_offset,
                },
            ) => a_bounds == b_bounds && a_offset == b_offset,
            (Self::ModifiersChanged(a), Self::ModifiersChanged(b)) => a == b,
            (Self::PointerMoved(a), Self::PointerMoved(b)) => a == b,
            (Self::ModalScrimPressed, Self::ModalScrimPressed) => true,
            (Self::WheelScrolled(a), Self::WheelScrolled(b)) => a == b,
            (Self::SelectTool(a), Self::SelectTool(b)) => a == b,
            (Self::Undo, Self::Undo) => true,
            (Self::Redo, Self::Redo) => true,
            (Self::DeleteSelected, Self::DeleteSelected) => true,
            (Self::EscapePressed, Self::EscapePressed) => true,
            (Self::ToggleNavigator, Self::ToggleNavigator) => true,
            (Self::NavigatorJump(a), Self::NavigatorJump(b)) => a == b,
            (Self::ToggleCopyMenu, Self::ToggleCopyMenu) => true,
            (Self::ToggleMoreMenu, Self::ToggleMoreMenu) => true,
            (Self::CanvasPressed(a), Self::CanvasPressed(b)) => a == b,
            (Self::CanvasMoved(a), Self::CanvasMoved(b)) => a == b,
            (Self::CanvasReleased(a), Self::CanvasReleased(b)) => a == b,
            (Self::TextDraftAction(a), Self::TextDraftAction(b)) => {
                std::mem::discriminant(a) == std::mem::discriminant(b)
            }
            (Self::CommitTextDraft, Self::CommitTextDraft) => true,
            (Self::ConfirmUnredactedAction, Self::ConfirmUnredactedAction) => true,
            (Self::CancelUnredactedAction, Self::CancelUnredactedAction) => true,
            (Self::SmartRedaction, Self::SmartRedaction) => true,
            (Self::ExportBugReport, Self::ExportBugReport) => true,
            (Self::IssuePackReviewChanged(a), Self::IssuePackReviewChanged(b)) => a == b,
            (Self::IssuePackReviewRedactions, Self::IssuePackReviewRedactions) => true,
            (Self::IssuePackExportFolder, Self::IssuePackExportFolder) => true,
            (Self::IssuePackExportZip, Self::IssuePackExportZip) => true,
            (Self::IssuePackFolderChosen(a), Self::IssuePackFolderChosen(b)) => a == b,
            (Self::IssuePackFinished(a), Self::IssuePackFinished(b)) => a == b,
            (Self::IssuePackCancel, Self::IssuePackCancel) => true,
            (Self::Workbench(a), Self::Workbench(b)) => {
                std::mem::discriminant(a) == std::mem::discriminant(b)
            }
            (Self::NextNumberInputChanged(a), Self::NextNumberInputChanged(b)) => a == b,
            (Self::CommitNextNumber, Self::CommitNextNumber) => true,
            (Self::OpenColorPicker(a), Self::OpenColorPicker(b)) => a == b,
            (Self::PreviewColor(a), Self::PreviewColor(b)) => a == b,
            (Self::ColorHexChanged(a), Self::ColorHexChanged(b)) => a == b,
            (Self::ApplyColor, Self::ApplyColor) => true,
            (Self::CancelColor, Self::CancelColor) => true,
            (Self::PreviewStrokeWidth(a), Self::PreviewStrokeWidth(b)) => a == b,
            (Self::ApplyStrokeWidth, Self::ApplyStrokeWidth) => true,
            (Self::CancelStrokeWidth, Self::CancelStrokeWidth) => true,
            (Self::PreviewStrokeOpacity(a), Self::PreviewStrokeOpacity(b)) => {
                a.to_bits() == b.to_bits()
            }
            (Self::ApplyStrokeOpacity, Self::ApplyStrokeOpacity) => true,
            (Self::CancelStrokeOpacity, Self::CancelStrokeOpacity) => true,
            (Self::SetNumberSize(a), Self::SetNumberSize(b)) => a == b,
            (Self::SetTextSize(a), Self::SetTextSize(b)) => a == b,
            (Self::ToggleTextBackground, Self::ToggleTextBackground) => true,
            (Self::PreviewShapeStrokeWidth(a), Self::PreviewShapeStrokeWidth(b)) => a == b,
            (Self::ToggleShapeFill, Self::ToggleShapeFill) => true,
            (Self::ApplyShapeStyle, Self::ApplyShapeStyle) => true,
            (Self::CancelShapeStyle, Self::CancelShapeStyle) => true,
            (Self::PreviewPixelateBlockSize(a), Self::PreviewPixelateBlockSize(b)) => a == b,
            (Self::ApplyPixelateBlockSize, Self::ApplyPixelateBlockSize) => true,
            (Self::CancelPixelateBlockSize, Self::CancelPixelateBlockSize) => true,
            (Self::ToggleShapesMenu, Self::ToggleShapesMenu) => true,
            (Self::SelectShape(a), Self::SelectShape(b)) => a == b,
            (Self::SelectRememberedShape, Self::SelectRememberedShape) => true,
            (
                Self::PixelatePreviewReady(a_req, a_res),
                Self::PixelatePreviewReady(b_req, b_res),
            ) => {
                a_req.generation == b_req.generation
                    && a_req.key == b_req.key
                    && match (a_res, b_res) {
                        (Ok(a), Ok(b)) => a.width == b.width && a.height == b.height,
                        (Err(_), Err(_)) => true,
                        _ => false,
                    }
            }
            #[cfg(feature = "ocr")]
            (Self::OcrPrepared(a), Self::OcrPrepared(b)) => a == b,
            #[cfg(feature = "ocr")]
            (Self::OcrSelectionStarted(a), Self::OcrSelectionStarted(b)) => a == b,
            #[cfg(feature = "ocr")]
            (Self::OcrSelectionChanged(a), Self::OcrSelectionChanged(b)) => a == b,
            #[cfg(feature = "ocr")]
            (Self::OcrSelectionFinished(a), Self::OcrSelectionFinished(b)) => a == b,
            #[cfg(feature = "ocr")]
            (Self::SelectAllOcrText, Self::SelectAllOcrText) => true,
            #[cfg(feature = "ocr")]
            (Self::CopyOcrSelection, Self::CopyOcrSelection) => true,
            #[cfg(feature = "ocr")]
            (Self::CopyAllOcrText, Self::CopyAllOcrText) => true,
            #[cfg(feature = "ocr")]
            (Self::CopyOcrFinished(a), Self::CopyOcrFinished(b)) => a == b,
            _ => false,
        }
    }
}

// ---------------------------------------------------------------------------
// Gesture helpers
// ---------------------------------------------------------------------------

#[cfg(feature = "ocr")]
fn redactions(document: &ImageDocument) -> Vec<Annotation> {
    document
        .annotations()
        .iter()
        .filter(|annotation| matches!(annotation, Annotation::OpaqueRedaction { .. }))
        .cloned()
        .collect()
}

#[cfg(feature = "ocr")]
fn prepare_ocr_task(state: &mut super::ResultWorkspace) -> Task<Message> {
    state.ocr_text.begin_prepare();
    let image = state.document.image.source().clone();
    Task::perform(
        async move {
            tokio::task::spawn_blocking(move || crate::product_ocr::prepare(&image))
                .await
                .unwrap_or(Err(crate::product_ocr::ProductOcrError::Detect))
        },
        Message::OcrPrepared,
    )
}

#[cfg(feature = "ocr")]
fn ocr_ready_message(document: &super::ocr_text::OcrTextDocument) -> String {
    let visible_count = document.visible_items().len();
    match visible_count {
        0 => "OCR complete: no visible text found".to_string(),
        1 => "OCR complete: 1 text block ready".to_string(),
        n => format!("OCR complete: {n} text blocks ready"),
    }
}

fn current_scale(state: &super::ResultWorkspace) -> f32 {
    geometry_for(
        state.viewport.zoom,
        state.original_size(),
        state.viewport_bounds,
    )
    .scale
}

fn clear_property_transactions(state: &mut super::ResultWorkspace) {
    state.editor.properties.color = None;
    state.editor.properties.width = None;
    state.editor.properties.opacity = None;
    state.editor.properties.shape_style = None;
    state.editor.properties.block_size = None;
    if state.editor.properties.popup == Some(super::properties::Popup::ColorPicker) {
        state.editor.properties.popup = None;
    }
}

fn set_selection(state: &mut super::ResultWorkspace, selection: Option<AnnotationId>) {
    if state.editor.selection != selection {
        clear_property_transactions(state);
        state.editor.selection = selection;
        state.editor.shapes_menu_open = false;
    }
}

fn active_two_point(state: &super::ResultWorkspace) -> Option<(TwoPointKind, StrokeStyle)> {
    match state.editor.tool {
        Tool::Line => Some((TwoPointKind::Line, state.annotation_defaults.values.line)),
        Tool::Arrow => Some((TwoPointKind::Arrow, state.annotation_defaults.values.arrow)),
        _ => None,
    }
}

fn active_freehand(
    state: &super::ResultWorkspace,
) -> Option<(rollshot_image_document::FreehandKind, StrokeStyle)> {
    match state.editor.tool {
        Tool::Pen => Some((
            rollshot_image_document::FreehandKind::Pen,
            state.annotation_defaults.values.pen,
        )),
        Tool::Highlighter => Some((
            rollshot_image_document::FreehandKind::Highlighter,
            state.annotation_defaults.values.highlighter,
        )),
        _ => None,
    }
}

fn prepare_shape_transaction(
    kind: rollshot_image_document::ShapeKind,
    id: Option<AnnotationId>,
    original_stroke: StrokeStyle,
    original_fill: Option<Rgb8>,
    remembered: Rgb8,
) -> super::properties::ShapeStyleTransaction {
    super::properties::ShapeStyleTransaction {
        id: id.unwrap_or(rollshot_image_document::AnnotationId(u64::MAX)),
        kind,
        original_stroke,
        original_fill,
        preview_stroke: original_stroke,
        preview_fill: original_fill,
        remembered_fill_color: remembered,
    }
}

macro_rules! resolve_shape_transaction {
    ($state:expr, $target:expr) => {{
        let (kind, id, original_stroke, original_fill, remembered) = match $target {
            super::properties::PropertyTarget::ShapeTool(kind) => {
                let sd = $state.annotation_defaults.values.shape(kind);
                (
                    kind,
                    None,
                    sd.stroke,
                    if sd.fill_enabled {
                        Some(sd.fill_color)
                    } else {
                        None
                    },
                    sd.fill_color,
                )
            }
            super::properties::PropertyTarget::Annotation(id) => {
                match $state.document.image.annotation(id) {
                    Some(Annotation::Shape {
                        kind, stroke, fill, ..
                    }) => {
                        let remembered = $state.annotation_defaults.values.shape(*kind).fill_color;
                        (*kind, Some(id), *stroke, *fill, remembered)
                    }
                    _ => return Task::none(),
                }
            }
            _ => return Task::none(),
        };
        let new_tx =
            prepare_shape_transaction(kind, id, original_stroke, original_fill, remembered);
        let tx = $state.editor.properties.shape_style.get_or_insert(new_tx);
        if id.is_some() && tx.id != id.unwrap() {
            *tx = prepare_shape_transaction(kind, id, original_stroke, original_fill, remembered);
        }
        tx
    }};
}

fn grab_offset(annotation: &Annotation, part: HitPart, point: ImagePoint) -> (f32, f32) {
    match (annotation, part) {
        (Annotation::TwoPoint { start, .. }, HitPart::Body) => {
            (point.x - start.x, point.y - start.y)
        }
        (Annotation::TextNote { position, .. }, HitPart::Body) => {
            (point.x - position.x, point.y - position.y)
        }
        (Annotation::OpaqueRedaction { bounds, .. }, HitPart::Body) => {
            (point.x - bounds.x, point.y - bounds.y)
        }
        (Annotation::NumberCallout { bubble, .. }, HitPart::Body) => {
            (point.x - bubble.x, point.y - bubble.y)
        }
        (Annotation::Shape { bounds, .. }, HitPart::Body) => {
            (point.x - bounds.x, point.y - bounds.y)
        }
        (Annotation::Pixelate { bounds, .. }, HitPart::Body) => {
            (point.x - bounds.x, point.y - bounds.y)
        }
        (Annotation::Freehand { points, .. }, HitPart::Body) => {
            (point.x - points[0].x, point.y - points[0].y)
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
        Tool::Number
        | Tool::Text
        | Tool::Line
        | Tool::Arrow
        | Tool::Rectangle
        | Tool::Ellipse
        | Tool::Pen
        | Tool::Highlighter
        | Tool::Pixelate => None,
        #[cfg(feature = "ocr")]
        Tool::OcrText => None,
    }
}

// ---------------------------------------------------------------------------
// Payload helpers
// ---------------------------------------------------------------------------

/// The image Copy places on the clipboard: always the flattened document
/// (pixel-identical to the source when no annotations exist — spec §12.1).
pub(crate) fn copy_payload(state: &super::ResultWorkspace) -> RgbaImage {
    let started = std::time::Instant::now();
    let out = state.document.image.flatten();
    tracing::debug!(
        target: "rollshot::annotation",
        elapsed_ms = started.elapsed().as_millis() as u64,
        annotations = state.document.image.annotations().len(),
        "flatten for copy"
    );
    out
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
        let started = std::time::Instant::now();
        let out = state.document.image.flatten();
        tracing::debug!(
            target: "rollshot::annotation",
            elapsed_ms = started.elapsed().as_millis() as u64,
            annotations = state.document.image.annotations().len(),
            "flatten for save"
        );
        out
    }
}

#[cfg(feature = "ocr")]
fn copy_ocr_text_task(text: String) -> Task<Message> {
    iced::clipboard::write(text).chain(Task::done(Message::CopyOcrFinished(Ok(()))))
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
    state.editor.more_menu_open = false;
    state.editor.shapes_menu_open = false;
    state.editor.properties.popup = None;

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
                        set_selection(state, Some(hit.id));
                        let existing_style = match state.document.image.annotation(hit.id) {
                            Some(Annotation::TextNote { style, .. }) => *style,
                            _ => state.annotation_defaults.values.text,
                        };
                        state.editor.text_draft = Some(TextDraft {
                            target: Some(hit.id),
                            position: *position,
                            content: iced::widget::text_editor::Content::with_text(text),
                            style: existing_style,
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
                    set_selection(state, Some(hit.id));
                    state.editor.drag = Some(DragState::EditAnnotation {
                        part: hit.part,
                        grab_offset: grab_offset(&original, hit.part, point),
                        raw_point: point,
                        current: original.clone(),
                        original,
                    });
                }
                None => {
                    set_selection(state, None);
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
                style: state.annotation_defaults.values.text,
            });
            iced::widget::operation::focus(state.text_editor_id.clone())
        }
        Tool::Line | Tool::Arrow => {
            let (kind, style) = active_two_point(state)
                .expect("line and arrow tools always provide two-point defaults");
            state.editor.drag = Some(DragState::CreateTwoPoint {
                kind,
                start: point,
                raw_current: point,
                style,
            });
            Task::none()
        }
        Tool::Rectangle | Tool::Ellipse => {
            let kind = if state.editor.tool == Tool::Rectangle {
                rollshot_image_document::ShapeKind::Rectangle
            } else {
                rollshot_image_document::ShapeKind::Ellipse
            };
            let shape_defaults = state.annotation_defaults.values.shape(kind);
            state.editor.drag = Some(DragState::CreateShape {
                kind,
                anchor: point,
                current: point,
                style: shape_defaults.stroke,
                fill: if shape_defaults.fill_enabled {
                    Some(shape_defaults.fill_color)
                } else {
                    None
                },
            });
            Task::none()
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
                    raw_point: point,
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
        Tool::Pixelate => {
            state.editor.drag = Some(DragState::CreatePixelate {
                anchor: point,
                current: point,
            });
            Task::none()
        }
        #[cfg(feature = "ocr")]
        Tool::OcrText => Task::none(),
        Tool::Pen | Tool::Highlighter => {
            let (kind, style) = active_freehand(state)
                .expect("pen and highlighter tools always provide freehand defaults");
            let (w, h) = state.document.image.source().dimensions();
            state.editor.drag = Some(DragState::CreateFreehand {
                kind,
                points: vec![point.clamp_to(w, h)],
                style,
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
    let shift = state.modifiers.shift();
    let scale = current_scale(state);
    match &mut state.editor.drag {
        Some(DragState::CreateNumber { bubble, .. }) => {
            *bubble = point;
            Task::none()
        }
        Some(DragState::CreateTwoPoint { raw_current, .. }) => {
            *raw_current = point;
            Task::none()
        }
        Some(DragState::CreateRedaction { current, .. }) => {
            *current = point;
            Task::none()
        }
        Some(DragState::CreatePixelate { current, .. }) => {
            *current = point;
            Task::none()
        }
        Some(DragState::CreateShape { current, .. }) => {
            *current = point;
            Task::none()
        }
        Some(DragState::CreateFreehand { points, .. }) => {
            if let Some(last) = points.last().copied() {
                if super::freehand_tool::should_accept_point(last, point, scale) {
                    points.push(point);
                }
            }
            Task::none()
        }
        Some(DragState::EditAnnotation {
            part,
            original,
            grab_offset,
            raw_point,
            current,
        }) => {
            *raw_point = point;
            *current =
                dragged_annotation(original, *part, point, *grab_offset, shift, (w, h), scale);
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
    let shift = state.modifiers.shift();
    let scale = current_scale(state);
    match state.editor.drag.take() {
        Some(DragState::CreateNumber { tip, .. }) => {
            let id = state.document.image.add_number_callout_with_style(
                tip,
                point,
                state.annotation_defaults.values.number,
            );
            set_selection(state, Some(id));
        }
        Some(DragState::CreateTwoPoint {
            kind, start, style, ..
        }) => {
            let raw_current = point;
            let end = bounded_constrained_endpoint(start, raw_current, shift, w, h);
            if gesture_meets_threshold(start, end, scale) {
                if let Err(error) = state
                    .document
                    .image
                    .add_two_point_with_style(kind, start, end, style)
                {
                    state.message = Some(InlineMessage::Error(error.to_string()));
                }
            }
        }
        Some(DragState::CreateRedaction { anchor, .. }) => {
            if let Ok(id) = state
                .document
                .image
                .add_redaction(ImageRect::from_corners(anchor, point))
            {
                set_selection(state, Some(id));
            }
        }
        Some(DragState::CreatePixelate { anchor, .. }) => {
            let bounds = super::box_tool::creation_bounds(anchor, point, shift, w, h);
            if super::box_tool::meets_creation_threshold(bounds, scale) {
                let block_size = state.annotation_defaults.values.pixelate_block_size;
                let _ = state.document.image.add_pixelate(bounds, block_size);
            }
        }
        Some(DragState::CreateShape {
            kind,
            anchor,
            style,
            fill,
            ..
        }) => {
            let bounds = super::box_tool::creation_bounds(anchor, point, shift, w, h);
            if super::box_tool::meets_creation_threshold(bounds, scale) {
                if let Err(e) = state
                    .document
                    .image
                    .add_shape_with_style(kind, bounds, style, fill)
                {
                    state.message = Some(InlineMessage::Error(e.to_string()));
                }
            }
        }
        Some(DragState::CreateFreehand {
            kind,
            mut points,
            style,
        }) => {
            if points
                .last()
                .is_some_and(|last| super::freehand_tool::should_accept_point(*last, point, scale))
            {
                points.push(point);
            }
            let input_points = points.len();
            let epsilon = super::freehand_tool::RDP_EPSILON_SCREEN / scale;
            let simplified = super::freehand_tool::simplify_rdp(&points, epsilon);
            tracing::debug!(
                target: "rollshot::annotation",
                input_points,
                output_points = simplified.len(),
                kind = ?kind,
                "freehand simplification"
            );
            if super::freehand_tool::path_meets_threshold(&simplified, scale) {
                if let Err(error) = state
                    .document
                    .image
                    .add_freehand_with_style(kind, simplified, style)
                {
                    state.message = Some(InlineMessage::Error(error.to_string()));
                }
            }
        }
        Some(DragState::EditAnnotation {
            part,
            original,
            grab_offset,
            ..
        }) => {
            let raw_point = point;
            let current = dragged_annotation(
                &original,
                part,
                raw_point,
                grab_offset,
                shift,
                (w, h),
                scale,
            );
            if current != original {
                let result = match &current {
                    Annotation::TwoPoint { start, end, .. } => state
                        .document
                        .image
                        .set_two_point_points(original.id(), *start, *end),
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
                    Annotation::Shape { bounds, .. } => state
                        .document
                        .image
                        .set_shape_bounds(original.id(), *bounds),
                    Annotation::Freehand { points, .. } => state
                        .document
                        .image
                        .set_freehand_points(original.id(), points.clone()),
                    Annotation::Pixelate { bounds, .. } => state
                        .document
                        .image
                        .set_pixelate_bounds(original.id(), *bounds),
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
    #[cfg(feature = "ocr")]
    refresh_ocr_redaction_mask(state);
    let scheduling = schedule_pixelate_previews(state);
    task.chain(scheduling)
}

fn refresh_navigator(state: &mut super::ResultWorkspace) {
    let current = state.document.image.state_id();
    if state.editor.navigator_items_state != Some(current) {
        state.editor.navigator_items = state.document.image.navigator_items();
        state.editor.navigator_items_state = Some(current);
    }
}

#[cfg(feature = "ocr")]
fn refresh_ocr_redaction_mask(state: &mut super::ResultWorkspace) {
    let redactions = redactions(&state.document.image);
    state.ocr_text.refresh_redactions(&redactions);
}

/// Collect requested pixelate preview keys, evict stale cache entries, and
/// spawn workers for missing keys that are not already in-flight.
fn schedule_pixelate_previews(state: &mut super::ResultWorkspace) -> Task<Message> {
    let scale = current_scale(state);
    let geometry = super::viewport::geometry_for(
        state.viewport.zoom,
        state.original_size(),
        state.viewport_bounds,
    );
    let visible = super::canvas::visible_image_rect(
        state.viewport.scroll_offset,
        state.viewport_bounds,
        geometry.scale,
        geometry.image_origin,
    );

    // Collect transient draft/property/direct-manipulation Pixelate annotations.
    let transient: Vec<Annotation> = {
        let mut t = Vec::new();
        if let Some(draft) = draft_pixelate_annotation(state) {
            t.push(draft);
        }
        if let Some(ref preview) = super::properties::preview_annotation(state) {
            if matches!(preview, Annotation::Pixelate { .. }) {
                t.push(preview.clone());
            }
        }
        t
    };

    let keys = requested_pixelate_keys(&state.document.image, &transient, visible, scale);
    state.pixelate_previews.retain_requested(&keys);

    let source = state.document.image.shared_source();
    let mut tasks = Vec::new();
    for key in keys {
        if state.pixelate_previews.lookup(key).is_some() {
            continue;
        }
        let Some(request) = state.pixelate_previews.begin_request(key) else {
            continue;
        };
        let src = source.clone();
        let req_for_message = request.clone();
        tasks.push(iced::Task::perform(
            async move {
                match tokio::task::spawn_blocking(move || generate_preview(src, request)).await {
                    Ok(result) => result,
                    Err(_) => Err(super::pixelate_preview::PreviewGenerationError::WorkerFailed),
                }
            },
            move |result| Message::PixelatePreviewReady(req_for_message, result),
        ));
    }

    if tasks.is_empty() {
        Task::none()
    } else {
        Task::batch(tasks)
    }
}

/// Extract the current Pixelate draft annotation from the editor drag state.
fn draft_pixelate_annotation(state: &super::ResultWorkspace) -> Option<Annotation> {
    let (w, h) = state.document.image.source().dimensions();
    match &state.editor.drag {
        Some(super::canvas::DragState::CreatePixelate { anchor, current }) => {
            let bounds =
                super::box_tool::creation_bounds(*anchor, *current, state.modifiers.shift(), w, h);
            (!bounds.is_empty()).then_some(Annotation::pixelate(
                AnnotationId(u64::MAX),
                bounds,
                state.annotation_defaults.values.pixelate_block_size,
            ))
        }
        Some(super::canvas::DragState::EditAnnotation { current, .. }) => {
            matches!(current, Annotation::Pixelate { .. }).then(|| current.clone())
        }
        _ => None,
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
            let result = crate::image_clipboard::copy_rgba_image(&copy_payload(state));
            Task::done(Message::CopyFinished {
                result,
                safe_output,
            })
        }
        Message::KeyboardCopy => {
            #[cfg(feature = "ocr")]
            if state.editor.tool == Tool::OcrText {
                return update_inner(state, Message::CopyOcrSelection);
            }
            update_inner(state, Message::Copy)
        }
        Message::CopyOriginal => {
            state.editor.copy_menu_open = false;
            state.editor.properties.popup = None;
            commit_text_draft(state);
            if super::secure_sharing::has_secure_redactions(&state.document) {
                state.pending_unredacted_action =
                    Some(super::secure_sharing::UnredactedAction::CopyOriginal);
                Task::none()
            } else {
                let result = crate::image_clipboard::copy_rgba_image(&copy_original_payload(state));
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
            let default_dir = state.document.default_save_dir().unwrap_or_else(|| {
                crate::storage::Platform::current()
                    .and_then(crate::storage::default_output_dir)
                    .unwrap_or_else(|_| PathBuf::from("."))
            });
            let default_name = super::secure_sharing::default_save_name(&state.document);
            Task::perform(
                super::actions::prompt_save_as(default_dir, default_name),
                Message::SavePathChosen,
            )
        }
        Message::SavePathChosen(Some(path)) => {
            let path = match super::actions::normalize_png_destination(path) {
                Ok(path) => path,
                Err(error) => {
                    state.message = Some(InlineMessage::Error(error));
                    return Task::none();
                }
            };
            if let Err(error) =
                super::secure_sharing::validate_export_destination(&state.document, &path)
            {
                state.message = Some(InlineMessage::Error(error.message().to_string()));
                return Task::none();
            }
            let safe_output = state.has_secure_redactions();
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
            state.editor.more_menu_open = false;
            state.editor.properties.popup = None;
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
            let image_size = state.document.image.source().dimensions();
            let scale = current_scale(state);
            if let Some(DragState::EditAnnotation {
                part,
                original,
                grab_offset,
                raw_point,
                current,
            }) = &mut state.editor.drag
            {
                *current = dragged_annotation(
                    original,
                    *part,
                    *raw_point,
                    *grab_offset,
                    modifiers.shift(),
                    image_size,
                    scale,
                );
            }
            Task::none()
        }
        Message::PointerMoved(position) => {
            state.pointer_position = position;
            Task::none()
        }
        Message::ModalScrimPressed => Task::none(),
        Message::WheelScrolled(delta) => handle_wheel(state, delta),
        Message::SelectTool(tool) => {
            if state.editor.properties.focus.is_some() {
                return Task::none();
            }
            #[cfg(feature = "ocr")]
            if state.editor.tool == Tool::OcrText && tool != Tool::OcrText {
                return Task::none();
            }
            commit_text_draft(state);
            state.editor.more_menu_open = false;
            state.editor.shapes_menu_open = false;
            state.editor.properties.popup = None;
            state.editor.properties.color = None;
            state.editor.properties.width = None;
            state.editor.properties.opacity = None;
            state.editor.properties.shape_style = None;
            state.editor.properties.block_size = None;
            #[cfg(feature = "ocr")]
            if tool == Tool::OcrText {
                state.editor.drag = None;
                set_selection(state, None);
                state.editor.tool = Tool::OcrText;
                if state.ocr_text.document().is_none() {
                    return prepare_ocr_task(state);
                }
                return Task::none();
            }
            state.editor.tool = tool;
            state.editor.drag = None;
            Task::none()
        }
        Message::Undo => {
            if state.editor.properties.focus.is_some() {
                return Task::none();
            }
            #[cfg(feature = "ocr")]
            if state.editor.tool == Tool::OcrText {
                return Task::none();
            }
            if state.editor.properties.color.is_some()
                || state.editor.properties.width.is_some()
                || state.editor.properties.opacity.is_some()
                || state.editor.properties.shape_style.is_some()
                || state.editor.properties.block_size.is_some()
            {
                state.editor.properties.color = None;
                state.editor.properties.width = None;
                state.editor.properties.opacity = None;
                state.editor.properties.shape_style = None;
                state.editor.properties.block_size = None;
                state.editor.properties.popup = None;
                return Task::none();
            }
            commit_text_draft(state);
            let _ = state.document.image.undo();
            prune_stale_selection(state);
            Task::none()
        }
        Message::Redo => {
            if state.editor.properties.focus.is_some() {
                return Task::none();
            }
            #[cfg(feature = "ocr")]
            if state.editor.tool == Tool::OcrText {
                return Task::none();
            }
            if state.editor.properties.color.is_some()
                || state.editor.properties.width.is_some()
                || state.editor.properties.opacity.is_some()
                || state.editor.properties.shape_style.is_some()
                || state.editor.properties.block_size.is_some()
            {
                state.editor.properties.color = None;
                state.editor.properties.width = None;
                state.editor.properties.opacity = None;
                state.editor.properties.shape_style = None;
                state.editor.properties.block_size = None;
                state.editor.properties.popup = None;
                return Task::none();
            }
            commit_text_draft(state);
            let _ = state.document.image.redo();
            prune_stale_selection(state);
            Task::none()
        }
        Message::DeleteSelected => {
            if state.editor.properties.focus.is_some() {
                return Task::none();
            }
            if state.editor.text_draft.is_some() {
                return Task::none();
            }
            let selected = state.editor.selection;
            set_selection(state, None);
            if let Some(id) = selected {
                let _ = state.document.image.delete_annotation(id);
            }
            Task::none()
        }
        Message::EscapePressed => {
            #[cfg(feature = "ocr")]
            if state.editor.tool == Tool::OcrText {
                if state.ocr_text.selection().is_some() {
                    state.ocr_text.set_selection(None);
                    return Task::none();
                }
                state.editor.tool = Tool::Select;
                return Task::none();
            }
            if state.editor.properties.color.is_some()
                || state.editor.properties.width.is_some()
                || state.editor.properties.opacity.is_some()
                || state.editor.properties.shape_style.is_some()
                || state.editor.properties.block_size.is_some()
            {
                state.editor.properties.color = None;
                state.editor.properties.width = None;
                state.editor.properties.opacity = None;
                state.editor.properties.shape_style = None;
                state.editor.properties.block_size = None;
                state.editor.properties.popup = None;
                return Task::none();
            }
            if state.pending_unredacted_action.is_some() {
                state.pending_unredacted_action = None;
            } else if state.editor.copy_menu_open {
                state.editor.copy_menu_open = false;
                state.editor.properties.popup = None;
            } else if state.editor.more_menu_open {
                state.editor.more_menu_open = false;
                state.editor.properties.popup = None;
            } else if state.editor.shapes_menu_open {
                state.editor.shapes_menu_open = false;
            } else if state.editor.text_draft.is_some() {
                state.editor.text_draft = None;
            } else if state.editor.drag.is_some() {
                state.editor.drag = None;
            } else if state.editor.selection.is_some() {
                set_selection(state, None);
            } else if state.editor.tool != Tool::Select {
                state.editor.tool = Tool::Select;
            } else {
                return update(state, Message::RequestClose);
            }
            Task::none()
        }
        Message::ToggleNavigator => {
            commit_text_draft(state);
            state.editor.more_menu_open = false;
            state.editor.shapes_menu_open = false;
            state.editor.properties.popup = None;
            state.editor.navigator_open = !state.editor.navigator_open;
            Task::none()
        }
        Message::NavigatorJump(id) => {
            commit_text_draft(state);
            if state.document.image.annotation(id).is_none() {
                set_selection(state, None);
                return Task::none();
            }
            set_selection(state, Some(id));
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
            state.editor.more_menu_open = false;
            state.editor.shapes_menu_open = false;
            state.editor.properties.color = None;
            state.editor.properties.popup = state
                .editor
                .copy_menu_open
                .then_some(super::properties::Popup::CopyMenu);
            Task::none()
        }
        Message::ToggleMoreMenu => {
            state.editor.more_menu_open = !state.editor.more_menu_open;
            state.editor.copy_menu_open = false;
            state.editor.shapes_menu_open = false;
            state.editor.properties.color = None;
            state.editor.properties.popup = state
                .editor
                .more_menu_open
                .then_some(super::properties::Popup::MoreMenu);
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
                let result = crate::image_clipboard::copy_rgba_image(&copy_original_payload(state));
                Task::done(Message::CopyFinished {
                    result,
                    safe_output: false,
                })
            }
            Some(super::secure_sharing::UnredactedAction::RevealOriginal) => {
                let Some(path) = state.document.source_path() else {
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
        #[cfg(feature = "ocr")]
        Message::OcrPrepared(Ok(items)) => {
            if state.editor.tool != Tool::OcrText {
                return Task::none();
            }
            let redactions = redactions(&state.document.image);
            state.ocr_text.finish_prepare(items, &redactions);
            if let Some(document) = state.ocr_text.document() {
                state.message = Some(InlineMessage::success(ocr_ready_message(document)));
            }
            Task::none()
        }
        #[cfg(feature = "ocr")]
        Message::OcrPrepared(Err(error)) => {
            state.ocr_text.fail_prepare(error);
            state.editor.tool = Tool::Select;
            state.message = Some(InlineMessage::Error(error.message().to_string()));
            Task::none()
        }
        #[cfg(feature = "ocr")]
        Message::OcrSelectionStarted(cursor) => {
            state
                .ocr_text
                .set_selection(Some(super::ocr_text::OcrSelection::range(cursor, cursor)));
            Task::none()
        }
        #[cfg(feature = "ocr")]
        Message::OcrSelectionChanged(cursor) | Message::OcrSelectionFinished(cursor) => {
            if let Some(selection) = state.ocr_text.selection().copied() {
                state
                    .ocr_text
                    .set_selection(Some(super::ocr_text::OcrSelection::range(
                        selection.anchor,
                        cursor,
                    )));
            }
            Task::none()
        }
        #[cfg(feature = "ocr")]
        Message::SelectAllOcrText => {
            if state.editor.tool != Tool::OcrText {
                return Task::none();
            }
            if let Some(document) = state.ocr_text.document() {
                state
                    .ocr_text
                    .set_selection(Some(super::ocr_text::OcrSelection::range(
                        super::ocr_text::TextCursor::new(0, 0),
                        document.end_cursor(),
                    )));
            }
            Task::none()
        }
        #[cfg(feature = "ocr")]
        Message::CopyOcrSelection => {
            let Some(document) = state.ocr_text.document() else {
                state.message = Some(InlineMessage::Error("No OCR text selected".into()));
                return Task::none();
            };
            let Some(selection) = state.ocr_text.selection() else {
                state.message = Some(InlineMessage::Error("No OCR text selected".into()));
                return Task::none();
            };
            let text = document.selected_text(selection);
            if text.is_empty() {
                state.message = Some(InlineMessage::Error("No OCR text selected".into()));
                return Task::none();
            }
            copy_ocr_text_task(text)
        }
        #[cfg(feature = "ocr")]
        Message::CopyAllOcrText => {
            let Some(document) = state.ocr_text.document() else {
                state.message = Some(InlineMessage::Error("No OCR text available".into()));
                return Task::none();
            };
            let text = document.copy_all_text();
            if text.is_empty() {
                state.message = Some(InlineMessage::Error("No OCR text available".into()));
                return Task::none();
            }
            copy_ocr_text_task(text)
        }
        #[cfg(feature = "ocr")]
        Message::CopyOcrFinished(Ok(())) => {
            state.message = Some(InlineMessage::success("Copied OCR text".into()));
            Task::none()
        }
        #[cfg(feature = "ocr")]
        Message::CopyOcrFinished(Err(error)) => {
            state.message = Some(InlineMessage::Error(error));
            Task::none()
        }
        Message::SmartRedaction => {
            state.editor.more_menu_open = false;
            state.editor.properties.popup = None;
            let mut wb = super::workbench::WorkbenchState::default();
            if let Ok(config_dir) = crate::daemon::config::rollshot_config_dir() {
                if let Ok(cfg) = super::workbench::load_provider_config(&config_dir) {
                    wb.provider_config = cfg;
                }
            }
            state.mode = super::workbench::WorkspaceMode::Workbench(wb);
            Task::none()
        }
        Message::ExportBugReport => {
            state.editor.more_menu_open = false;
            state.editor.properties.popup = None;
            if block_pending_candidates(state) {
                return Task::none();
            }
            commit_text_draft(state);
            state.issue_pack = Some(super::IssuePackDialog::new());
            state.message = None;
            Task::none()
        }
        Message::IssuePackReviewChanged(confirmed) => {
            if let Some(dialog) = &mut state.issue_pack {
                dialog.review_confirmed = confirmed;
            }
            Task::none()
        }
        Message::IssuePackReviewRedactions => {
            state.issue_pack = None;
            state.mode = super::workbench::WorkspaceMode::Normal;
            state.editor.tool = Tool::Redact;
            Task::none()
        }
        Message::IssuePackExportFolder => {
            begin_issue_pack_export(state, super::IssuePackKind::Folder)
        }
        Message::IssuePackExportZip => begin_issue_pack_export(state, super::IssuePackKind::Zip),
        Message::IssuePackFolderChosen(None) => {
            if let Some(dialog) = &mut state.issue_pack {
                dialog.pending_kind = None;
            }
            Task::none()
        }
        Message::IssuePackFolderChosen(Some(parent)) => {
            let kind = state
                .issue_pack
                .as_ref()
                .and_then(|dialog| dialog.pending_kind)
                .unwrap_or(super::IssuePackKind::Folder);
            let input = result_issue_pack_input(state);
            let result = match kind {
                super::IssuePackKind::Folder => crate::issue_pack::export_folder(&input, &parent),
                super::IssuePackKind::Zip => crate::issue_pack::export_zip(&input, &parent),
            };
            update_inner(
                state,
                Message::IssuePackFinished(result.map_err(|e| e.to_string())),
            )
        }
        Message::IssuePackFinished(Ok(result)) => {
            let mut text = match result.zip_path.as_ref() {
                Some(path) => format!("Exported bug report ZIP to {}", path.display()),
                None => format!("Exported bug report to {}", result.directory.display()),
            };
            if !result.warnings.is_empty() {
                let warning_text = result
                    .warnings
                    .iter()
                    .map(|warning| warning.message.as_str())
                    .collect::<Vec<_>>()
                    .join("; ");
                text = format!("{text}\nWarnings: {warning_text}");
            }
            state.issue_pack = None;
            state.message = Some(InlineMessage::success(text));
            Task::none()
        }
        Message::IssuePackFinished(Err(error)) => {
            if let Some(dialog) = &mut state.issue_pack {
                dialog.pending_kind = None;
            }
            state.message = Some(InlineMessage::Error(format!(
                "{error}\nIf the folder export succeeded, it is still available."
            )));
            Task::none()
        }
        Message::IssuePackCancel => {
            state.issue_pack = None;
            Task::none()
        }
        Message::Workbench(msg) => {
            let workbench = match &mut state.mode {
                super::workbench::WorkspaceMode::Workbench(wb) => wb,
                _ => return Task::none(),
            };
            match msg {
                super::workbench::WorkbenchMessage::RunEvent(event) => {
                    use rollshot_agent::runtime::RunEvent;
                    match &event {
                        RunEvent::TextChunk { text } => {
                            // Accumulate into the last AssistantText entry
                            // (spec §6.2 typewriter) instead of pushing one
                            // entry per chunk.
                            if let Some(super::workbench::state::ActivityEntry::AssistantText(
                                prev,
                            )) = workbench.live_activity.last_mut()
                            {
                                prev.push_str(text);
                            } else {
                                workbench.live_activity.push(
                                    super::workbench::state::ActivityEntry::AssistantText(
                                        text.clone(),
                                    ),
                                );
                            }
                        }
                        _ => {
                            if let Some(entry) =
                                super::workbench::state::event_to_activity_entry(&event)
                            {
                                workbench.live_activity.push(entry);
                            }
                        }
                    }
                    Task::none()
                }
                super::workbench::WorkbenchMessage::RunTerminal(terminal) => {
                    // Reconcile accumulated AssistantText against the
                    // authoritative final text before pushing the terminal
                    // label (spec §6.2 / addendum G — dropped try_send chunks
                    // can leave gaps in the streamed text).
                    let authoritative_text: Option<&str> = match &terminal {
                        rollshot_agent::driver::RunTerminalState::ReadyForReview(ready) => {
                            Some(&ready.assistant_text)
                        }
                        rollshot_agent::driver::RunTerminalState::NeedsUserInput(n) => {
                            Some(&n.assistant_text)
                        }
                        _ => None,
                    };
                    if let Some(final_text) = authoritative_text {
                        if !final_text.is_empty() {
                            let mut replaced = false;
                            for entry in workbench.live_activity.iter_mut().rev() {
                                if let super::workbench::state::ActivityEntry::AssistantText(prev) =
                                    entry
                                {
                                    *prev = final_text.to_string();
                                    replaced = true;
                                    break;
                                }
                            }
                            if !replaced {
                                workbench.live_activity.push(
                                    super::workbench::state::ActivityEntry::AssistantText(
                                        final_text.to_string(),
                                    ),
                                );
                            }
                        }
                    }
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
                    let (parent_revision_id, revision_note) = match &workbench.run_state {
                        super::workbench::RunState::Running {
                            parent_revision_id,
                            revision_note,
                            ..
                        } => (parent_revision_id.clone(), revision_note.clone()),
                        _ => (None, None),
                    };
                    workbench.run_state = super::workbench::RunState::Terminal(terminal);
                    if let super::workbench::RunState::Terminal(
                        rollshot_agent::driver::RunTerminalState::ReadyForReview(ref ready),
                    ) = &workbench.run_state
                    {
                        workbench.pending_proposal = Some(ready.proposal.clone());
                        let ids: Vec<_> = ready.proposal.candidates.iter().map(|c| c.id).collect();
                        workbench.review = super::workbench::CandidateReview::from_candidates(&ids);
                        workbench.selected_candidate = None;
                        // Fresh proposal: all candidates pending, no corrections yet.
                        workbench.corrections_non_empty = false;
                        workbench.pending_draft = Some(super::workbench::PendingDraft {
                            source: ready.automation.source.clone(),
                            assistant_text: ready.assistant_text.clone(),
                            validation_summary: ready.automation.validation_summary.clone(),
                            parent_revision_id,
                            revision_note,
                        });
                    }
                    Task::none()
                }
                super::workbench::WorkbenchMessage::RunFailed(e) => {
                    workbench.error = Some(e);
                    workbench.run_state = super::workbench::RunState::Terminal(
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
                                workbench.corrections_non_empty = false;
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
                    workbench.recompute_corrections_non_empty();
                    Task::none()
                }
                super::workbench::WorkbenchMessage::CandidateUnrejected(id) => {
                    workbench.review.mark_pending(id);
                    workbench.recompute_corrections_non_empty();
                    Task::none()
                }
                super::workbench::WorkbenchMessage::CandidateMoved { id, new_bounds } => {
                    workbench.review.mark_modified(
                        id,
                        rollshot_edit_proposal::ProposedEdit::AddRedaction { bounds: new_bounds },
                    );
                    workbench.recompute_corrections_non_empty();
                    Task::none()
                }
                super::workbench::WorkbenchMessage::AddManualCandidate { bounds } => {
                    let max_proposal_id = workbench
                        .pending_proposal
                        .as_ref()
                        .and_then(|proposal| proposal.candidates.iter().map(|c| c.id.0).max())
                        .unwrap_or(0);
                    let max_review_id = workbench
                        .review
                        .per_candidate
                        .keys()
                        .map(|id| id.0)
                        .max()
                        .unwrap_or(0);
                    let id = rollshot_edit_proposal::CandidateId(
                        max_proposal_id
                            .max(max_review_id)
                            .max(workbench.next_manual_candidate_id.saturating_sub(1))
                            + 1,
                    );
                    workbench.next_manual_candidate_id = id.0 + 1;
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
                    workbench.recompute_corrections_non_empty();
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
                        parent_revision_id: None,
                        revision_note: None,
                        preset_id: rollshot_preset::PresetId("workbench-draft".into()),
                        preset_store_root: crate::daemon::config::rollshot_config_dir()
                            .map(|dir| dir.join("presets"))
                            .unwrap_or_default(),
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
                    // Guard against concurrent runs (spec §4.5 freeze rule).
                    // The composer is disabled while Running, but this is a
                    // defense-in-depth check.
                    if workbench.run_state.is_running() {
                        return Task::none();
                    }
                    let Some(params) = workbench.pending_run.take() else {
                        return Task::none();
                    };
                    let parent_revision_id = params.parent_revision_id.clone();
                    let revision_note = params.revision_note.clone();
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
                            workbench.run_state = super::workbench::RunState::Running {
                                cancellation,
                                parent_revision_id,
                                revision_note,
                            };
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
                            let capability_bundle =
                                super::workbench::run::ProductCapabilityBundle::load(
                                    &store,
                                    Some(&preset_id),
                                )
                                .unwrap_or_else(|_| {
                                    super::workbench::run::ProductCapabilityBundle::empty()
                                });
                            let limits = rollshot_automation::ValidationLimits::default();
                            let metadata =
                                rollshot_automation::validate_source(&draft.source, &limits)
                                    .map(|validated| {
                                        super::workbench::run::revision_capability_metadata(
                                            &validated,
                                            &capability_bundle,
                                        )
                                    })
                                    .unwrap_or_default();
                            match super::workbench::review::save_revision_with_capabilities(
                                &store,
                                &preset_id,
                                &draft.source,
                                draft.parent_revision_id.as_ref(),
                                draft.revision_note.as_deref(),
                                workbench.session.session_id.get(),
                                chrono::Utc::now().to_rfc3339(),
                                metadata,
                            ) {
                                Ok(revision) => {
                                    workbench.active_revision = Some(revision);
                                    workbench.pending_draft = None;
                                }
                                Err(e) => workbench.error = Some(e),
                            }
                        } else {
                            workbench.error = Some(super::workbench::state::WorkbenchError::Config);
                        }
                    }
                    Task::none()
                }
                super::workbench::WorkbenchMessage::ImStart
                | super::workbench::WorkbenchMessage::AskAgentToRevise => {
                    if workbench.run_state.is_running() {
                        return Task::none();
                    }
                    let Some(active_revision) = workbench.active_revision.as_ref() else {
                        return Task::none();
                    };
                    let Some(proposal) = workbench.pending_proposal.as_ref() else {
                        return Task::none();
                    };
                    let evidence = super::workbench::review::assemble_correction_evidence(
                        proposal,
                        &workbench.review,
                    );
                    if evidence.is_empty() {
                        return Task::none();
                    }
                    let (w, h) = state.document.image.source().dimensions();
                    let summary = evidence.summary_line();
                    let params = super::workbench::PendingRunParams {
                        user_message: evidence.to_agent_message(),
                        image_dims: (w, h),
                        active_revision_source: Some(active_revision.artifact.source.clone()),
                        mode: super::workbench::RunKind::Improve,
                        parent_revision_id: Some(active_revision.id.clone()),
                        revision_note: Some(format!(
                            "improved from {}; {summary}",
                            active_revision.id.0
                        )),
                        preset_id: rollshot_preset::PresetId("workbench-draft".into()),
                        preset_store_root: crate::daemon::config::rollshot_config_dir()
                            .map(|dir| dir.join("presets"))
                            .unwrap_or_default(),
                    };
                    workbench.disclosure_pending = true;
                    workbench.pending_run = Some(params);
                    Task::none()
                }
                super::workbench::WorkbenchMessage::DiscardCandidates => {
                    workbench.pending_proposal = None;
                    workbench.review = super::workbench::CandidateReview::default();
                    workbench.selected_candidate = None;
                    workbench.corrections_non_empty = false;
                    Task::none()
                }
                super::workbench::WorkbenchMessage::DiscardDraft
                | super::workbench::WorkbenchMessage::ToggleAdvancedDetails
                | super::workbench::WorkbenchMessage::OpenProviderSettings
                | super::workbench::WorkbenchMessage::DisclosureRequested(_) => Task::none(),
            }
        }
        Message::NextNumberInputChanged(value) => {
            state.editor.properties.next_number_input = value;
            Task::none()
        }
        Message::CommitNextNumber => {
            let input = state.editor.properties.next_number_input.trim().to_string();
            match input.parse::<u32>() {
                Ok(n) => {
                    if let Err(e) = state.document.image.set_next_number(n) {
                        state.message = Some(InlineMessage::Error(e.to_string()));
                    } else {
                        state.editor.properties.next_number_input.clear();
                    }
                }
                Err(_) => {
                    if !input.is_empty() {
                        state.message = Some(InlineMessage::Error(
                            "Enter a valid positive integer".into(),
                        ));
                    }
                }
            }
            Task::none()
        }
        Message::OpenColorPicker(prop) => {
            use super::properties::{ColorProperty, ColorTransaction, PropertyTarget};
            let target = match super::properties::property_target(state) {
                Some(t) => t,
                None => return Task::none(),
            };
            if let PropertyTarget::Annotation(id) = target {
                if let Some(Annotation::Shape {
                    kind, stroke, fill, ..
                }) = state.document.image.annotation(id)
                {
                    let remembered = state.annotation_defaults.values.shape(*kind).fill_color;
                    let new_tx =
                        prepare_shape_transaction(*kind, Some(id), *stroke, *fill, remembered);
                    match &mut state.editor.properties.shape_style {
                        Some(tx) if tx.id == id => {}
                        slot => *slot = Some(new_tx),
                    }
                }
            }
            let original = match (&target, &prop) {
                (PropertyTarget::NumberTool, ColorProperty::NumberAccent) => {
                    state.annotation_defaults.values.number.accent
                }
                (PropertyTarget::TextTool, ColorProperty::TextColor) => {
                    state.annotation_defaults.values.text.text_color
                }
                (PropertyTarget::TextTool, ColorProperty::TextBackground) => state
                    .annotation_defaults
                    .values
                    .text
                    .background
                    .unwrap_or(rollshot_image_document::Rgb8::new(255, 255, 255)),
                (PropertyTarget::TwoPointTool(TwoPointKind::Line), ColorProperty::StrokeColor) => {
                    state.annotation_defaults.values.line.color
                }
                (PropertyTarget::TwoPointTool(TwoPointKind::Arrow), ColorProperty::StrokeColor) => {
                    state.annotation_defaults.values.arrow.color
                }
                (PropertyTarget::Annotation(id), ColorProperty::NumberAccent) => {
                    match state.document.image.annotation(*id) {
                        Some(Annotation::NumberCallout { style, .. }) => style.accent,
                        _ => return Task::none(),
                    }
                }
                (PropertyTarget::Annotation(id), ColorProperty::TextColor) => {
                    match state.document.image.annotation(*id) {
                        Some(Annotation::TextNote { style, .. }) => style.text_color,
                        _ => return Task::none(),
                    }
                }
                (PropertyTarget::Annotation(id), ColorProperty::TextBackground) => {
                    match state.document.image.annotation(*id) {
                        Some(Annotation::TextNote { style, .. }) => {
                            style.background.unwrap_or(Rgb8::new(255, 255, 255))
                        }
                        _ => return Task::none(),
                    }
                }
                (PropertyTarget::Annotation(id), ColorProperty::StrokeColor) => {
                    match state.document.image.annotation(*id) {
                        Some(Annotation::TwoPoint { style, .. }) => style.color,
                        Some(Annotation::Shape { .. }) => state
                            .editor
                            .properties
                            .shape_style
                            .as_ref()
                            .map(|tx| tx.preview_stroke.color)
                            .unwrap_or(StrokeStyle::default().color),
                        Some(Annotation::Freehand { style, .. }) => style.color,
                        _ => return Task::none(),
                    }
                }
                (PropertyTarget::ShapeTool(kind), ColorProperty::StrokeColor) => {
                    state.annotation_defaults.values.shape(*kind).stroke.color
                }
                (PropertyTarget::ShapeTool(kind), ColorProperty::ShapeFill) => {
                    state.annotation_defaults.values.shape(*kind).fill_color
                }
                (PropertyTarget::FreehandTool(kind), ColorProperty::StrokeColor) => match kind {
                    rollshot_image_document::FreehandKind::Pen => {
                        state.annotation_defaults.values.pen.color
                    }
                    rollshot_image_document::FreehandKind::Highlighter => {
                        state.annotation_defaults.values.highlighter.color
                    }
                },
                (PropertyTarget::Annotation(id), ColorProperty::ShapeFill) => {
                    match state.document.image.annotation(*id) {
                        Some(Annotation::Shape { .. }) => state
                            .editor
                            .properties
                            .shape_style
                            .as_ref()
                            .and_then(|tx| tx.preview_fill.or(Some(tx.remembered_fill_color)))
                            .unwrap_or(Rgb8::new(0xE5, 0x48, 0x4D)),
                        _ => return Task::none(),
                    }
                }
                _ => return Task::none(),
            };
            let hex = format!("#{:02X}{:02X}{:02X}", original.r, original.g, original.b);
            state.editor.properties.color = Some(ColorTransaction {
                target,
                property: prop,
                original,
                preview: original,
                hex,
            });
            state.editor.properties.width = None;
            state.editor.properties.opacity = None;
            state.editor.copy_menu_open = false;
            state.editor.more_menu_open = false;
            state.editor.shapes_menu_open = false;
            state.editor.properties.popup = Some(super::properties::Popup::ColorPicker);
            Task::none()
        }
        Message::PreviewColor(rgb) => {
            if let Some(tx) = &mut state.editor.properties.color {
                tx.preview = rgb;
                tx.hex = format!("#{:02X}{:02X}{:02X}", rgb.r, rgb.g, rgb.b);
                if let Some(shape) = &mut state.editor.properties.shape_style {
                    match tx.property {
                        super::properties::ColorProperty::StrokeColor => {
                            shape.preview_stroke.color = rgb;
                        }
                        super::properties::ColorProperty::ShapeFill => {
                            shape.preview_fill = Some(rgb);
                            shape.remembered_fill_color = rgb;
                        }
                        _ => {}
                    }
                }
            }
            Task::none()
        }
        Message::ColorHexChanged(input) => {
            if let Some(tx) = &mut state.editor.properties.color {
                if let Ok(rgb) = super::properties::parse_hex_rgb(&input) {
                    tx.preview = rgb;
                    if let Some(shape) = &mut state.editor.properties.shape_style {
                        match tx.property {
                            super::properties::ColorProperty::StrokeColor => {
                                shape.preview_stroke.color = rgb;
                            }
                            super::properties::ColorProperty::ShapeFill => {
                                shape.preview_fill = Some(rgb);
                                shape.remembered_fill_color = rgb;
                            }
                            _ => {}
                        }
                    }
                }
                tx.hex = input;
            }
            Task::none()
        }
        Message::ApplyColor => {
            let tx = match state.editor.properties.color.take() {
                Some(tx) => tx,
                None => return Task::none(),
            };
            state.editor.properties.popup = None;
            use super::properties::{ColorProperty, PropertyTarget};
            match tx.target {
                PropertyTarget::NumberTool => {
                    if let ColorProperty::NumberAccent = tx.property {
                        state.annotation_defaults.values.number.accent = tx.preview;
                    }
                    if let Some(path) = state.annotation_defaults.config_path.clone() {
                        if let Err(e) = super::annotation_defaults::save_to(
                            &path,
                            &state.annotation_defaults.values,
                        ) {
                            if !state.annotation_defaults.warning_reported {
                                state.annotation_defaults.warning_reported = true;
                                state.message =
                                    Some(InlineMessage::Warning(format!("Saved defaults: {e}")));
                            }
                        }
                    }
                }
                PropertyTarget::TextTool => {
                    match tx.property {
                        ColorProperty::TextColor => {
                            state.annotation_defaults.values.text.text_color = tx.preview;
                        }
                        ColorProperty::TextBackground => {
                            state.annotation_defaults.values.text.background = Some(tx.preview);
                        }
                        _ => {}
                    }
                    persist_annotation_defaults(state);
                }
                PropertyTarget::TwoPointTool(kind) => {
                    if tx.property == ColorProperty::StrokeColor {
                        match kind {
                            TwoPointKind::Line => {
                                state.annotation_defaults.values.line.color = tx.preview
                            }
                            TwoPointKind::Arrow => {
                                state.annotation_defaults.values.arrow.color = tx.preview
                            }
                        }
                        persist_annotation_defaults(state);
                    }
                }
                PropertyTarget::ShapeTool(kind) => {
                    let defaults = state.annotation_defaults.values.shape_mut(kind);
                    match tx.property {
                        ColorProperty::StrokeColor => defaults.stroke.color = tx.preview,
                        ColorProperty::ShapeFill => defaults.fill_color = tx.preview,
                        _ => {}
                    }
                    if matches!(
                        tx.property,
                        ColorProperty::StrokeColor | ColorProperty::ShapeFill
                    ) {
                        persist_annotation_defaults(state);
                    }
                }
                PropertyTarget::FreehandTool(kind) => {
                    if tx.property == ColorProperty::StrokeColor {
                        match kind {
                            rollshot_image_document::FreehandKind::Pen => {
                                state.annotation_defaults.values.pen.color = tx.preview;
                            }
                            rollshot_image_document::FreehandKind::Highlighter => {
                                state.annotation_defaults.values.highlighter.color = tx.preview;
                            }
                        }
                        persist_annotation_defaults(state);
                    }
                }
                PropertyTarget::Annotation(id) => match tx.property {
                    ColorProperty::NumberAccent => {
                        if let Some(Annotation::NumberCallout { style, .. }) =
                            state.document.image.annotation(id)
                        {
                            let mut new_style = *style;
                            new_style.accent = tx.preview;
                            if let Err(e) = state.document.image.set_number_style(id, new_style) {
                                state.message = Some(InlineMessage::Error(e.to_string()));
                            }
                        }
                    }
                    ColorProperty::TextColor => {
                        if let Some(Annotation::TextNote { style, .. }) =
                            state.document.image.annotation(id)
                        {
                            let mut new_style = *style;
                            new_style.text_color = tx.preview;
                            if let Err(e) = state.document.image.set_text_style(id, new_style) {
                                state.message = Some(InlineMessage::Error(e.to_string()));
                            }
                        }
                    }
                    ColorProperty::TextBackground => {
                        if let Some(Annotation::TextNote { style, .. }) =
                            state.document.image.annotation(id)
                        {
                            let mut new_style = *style;
                            new_style.background = Some(tx.preview);
                            if let Err(e) = state.document.image.set_text_style(id, new_style) {
                                state.message = Some(InlineMessage::Error(e.to_string()));
                            }
                        }
                    }
                    ColorProperty::StrokeColor => {
                        if let Some(
                            Annotation::TwoPoint { style, .. } | Annotation::Freehand { style, .. },
                        ) = state.document.image.annotation(id)
                        {
                            let mut new_style = *style;
                            new_style.color = tx.preview;
                            if let Err(e) = state.document.image.set_stroke_style(id, new_style) {
                                state.message = Some(InlineMessage::Error(e.to_string()));
                            }
                        }
                    }
                    ColorProperty::ShapeFill => {}
                },
                PropertyTarget::PixelateTool => {}
            }
            Task::none()
        }
        Message::PreviewShapeStrokeWidth(width) => {
            let Some(target) = super::properties::property_target(state) else {
                return Task::none();
            };
            let tx = resolve_shape_transaction!(state, target);
            tx.preview_stroke.width = width;
            Task::none()
        }
        Message::ToggleShapeFill => {
            let Some(target) = super::properties::property_target(state) else {
                return Task::none();
            };
            let tx = resolve_shape_transaction!(state, target);
            tx.preview_fill = if let Some(color) = tx.preview_fill {
                tx.remembered_fill_color = color;
                None
            } else {
                Some(tx.remembered_fill_color)
            };
            // Tool-default: persist immediately
            if let super::properties::PropertyTarget::ShapeTool(kind) = target {
                let sd = state.annotation_defaults.values.shape_mut(kind);
                sd.fill_enabled = tx.preview_fill.is_some();
                if let Some(color) = tx.preview_fill {
                    sd.fill_color = color;
                }
                state.editor.properties.shape_style = None;
                persist_annotation_defaults(state);
            }
            Task::none()
        }
        Message::ApplyShapeStyle => {
            let Some(tx) = state.editor.properties.shape_style.take() else {
                return Task::none();
            };
            state.editor.properties.color = None;
            state.editor.properties.popup = None;
            // For selected annotations: commit to document
            if tx.id.0 != u64::MAX {
                if let Err(e) =
                    state
                        .document
                        .image
                        .set_shape_style(tx.id, tx.preview_stroke, tx.preview_fill)
                {
                    state.message = Some(InlineMessage::Error(e.to_string()));
                }
            } else {
                state.annotation_defaults.values.shape_mut(tx.kind).stroke = tx.preview_stroke;
                persist_annotation_defaults(state);
            }
            Task::none()
        }
        Message::CancelShapeStyle => {
            state.editor.properties.shape_style = None;
            state.editor.properties.color = None;
            state.editor.properties.popup = None;
            Task::none()
        }
        Message::PreviewPixelateBlockSize(size) => {
            use super::properties::{BlockSizeTarget, BlockSizeTransaction};
            use rollshot_image_document::{MAX_PIXELATE_BLOCK_SIZE, MIN_PIXELATE_BLOCK_SIZE};
            let clamped = size.clamp(MIN_PIXELATE_BLOCK_SIZE, MAX_PIXELATE_BLOCK_SIZE);
            let target = match state.editor.tool {
                Tool::Pixelate => BlockSizeTarget::ToolDefault,
                Tool::Select => {
                    if let Some(id) = state.editor.selection {
                        BlockSizeTarget::Annotation(id)
                    } else {
                        return Task::none();
                    }
                }
                _ => return Task::none(),
            };
            let original = match target {
                BlockSizeTarget::ToolDefault => {
                    state.annotation_defaults.values.pixelate_block_size
                }
                BlockSizeTarget::Annotation(id) => state
                    .document
                    .image
                    .annotation(id)
                    .and_then(|a| match a {
                        Annotation::Pixelate { block_size, .. } => Some(*block_size),
                        _ => None,
                    })
                    .unwrap_or(state.annotation_defaults.values.pixelate_block_size),
            };
            match &mut state.editor.properties.block_size {
                Some(tx) if tx.target == target => {
                    tx.preview = clamped;
                }
                _ => {
                    state.editor.properties.block_size = Some(BlockSizeTransaction {
                        target,
                        original,
                        preview: clamped,
                    });
                }
            }
            Task::none()
        }
        Message::ApplyPixelateBlockSize => {
            use super::properties::BlockSizeTarget;
            let Some(tx) = state.editor.properties.block_size.take() else {
                return Task::none();
            };
            match tx.target {
                BlockSizeTarget::ToolDefault => {
                    state.annotation_defaults.values.pixelate_block_size = tx.preview;
                    persist_annotation_defaults(state);
                }
                BlockSizeTarget::Annotation(id) => {
                    if let Err(e) = state.document.image.set_pixelate_block_size(id, tx.preview) {
                        state.message = Some(InlineMessage::Error(e.to_string()));
                    }
                }
            }
            Task::none()
        }
        Message::CancelPixelateBlockSize => {
            state.editor.properties.block_size = None;
            Task::none()
        }
        Message::ToggleShapesMenu => {
            state.editor.shapes_menu_open = !state.editor.shapes_menu_open;
            state.editor.more_menu_open = false;
            state.editor.copy_menu_open = false;
            state.editor.properties.popup = None;
            Task::none()
        }
        Message::SelectShape(kind) => {
            let tool = match kind {
                rollshot_image_document::ShapeKind::Rectangle => Tool::Rectangle,
                rollshot_image_document::ShapeKind::Ellipse => Tool::Ellipse,
            };
            commit_text_draft(state);
            clear_property_transactions(state);
            state.editor.more_menu_open = false;
            state.editor.drag = None;
            state.editor.tool = tool;
            state.annotation_defaults.values.last_shape = kind;
            state.editor.shapes_menu_open = false;
            persist_annotation_defaults(state);
            Task::none()
        }
        Message::SelectRememberedShape => {
            let kind = state.annotation_defaults.values.last_shape;
            let tool = match kind {
                rollshot_image_document::ShapeKind::Rectangle => Tool::Rectangle,
                rollshot_image_document::ShapeKind::Ellipse => Tool::Ellipse,
            };
            commit_text_draft(state);
            clear_property_transactions(state);
            state.editor.more_menu_open = false;
            state.editor.shapes_menu_open = false;
            state.editor.drag = None;
            state.editor.tool = tool;
            Task::none()
        }
        Message::PixelatePreviewReady(request, result) => {
            use super::pixelate_preview::Completion;
            match result {
                Ok(pixels) => match state.pixelate_previews.complete(pixels) {
                    Completion::Accepted => {}
                    Completion::Stale => {}
                },
                Err(_) => {
                    let should_warn = state.pixelate_previews.fail(request.clone());
                    if should_warn {
                        state.message = Some(InlineMessage::Warning(
                            "Pixelate preview unavailable; showing an outline instead.".into(),
                        ));
                        tracing::warn!(
                            target: "rollshot::annotation",
                            source_id = request.key.source_id,
                            region_w = request.key.region.width,
                            region_h = request.key.region.height,
                            block_size = request.key.block_size,
                            "pixelate preview generation failed"
                        );
                    }
                }
            }
            Task::none()
        }
        Message::CancelColor => {
            if let Some(tx) = state.editor.properties.color.take() {
                if let Some(shape) = &mut state.editor.properties.shape_style {
                    match tx.property {
                        super::properties::ColorProperty::StrokeColor => {
                            shape.preview_stroke.color = tx.original;
                        }
                        super::properties::ColorProperty::ShapeFill => {
                            shape.preview_fill = Some(tx.original);
                            shape.remembered_fill_color = tx.original;
                        }
                        _ => {}
                    }
                }
            }
            state.editor.properties.popup = None;
            Task::none()
        }
        Message::PreviewStrokeWidth(width) => {
            use super::properties::{PropertyTarget, StrokeWidthTransaction};
            let Some(target) = super::properties::property_target(state) else {
                return Task::none();
            };
            let original = match target {
                PropertyTarget::TwoPointTool(TwoPointKind::Line) => {
                    state.annotation_defaults.values.line.width
                }
                PropertyTarget::TwoPointTool(TwoPointKind::Arrow) => {
                    state.annotation_defaults.values.arrow.width
                }
                PropertyTarget::FreehandTool(kind) => match kind {
                    rollshot_image_document::FreehandKind::Pen => {
                        state.annotation_defaults.values.pen.width
                    }
                    rollshot_image_document::FreehandKind::Highlighter => {
                        state.annotation_defaults.values.highlighter.width
                    }
                },
                PropertyTarget::Annotation(id) => match state.document.image.annotation(id) {
                    Some(Annotation::TwoPoint { style, .. }) => style.width,
                    Some(Annotation::Freehand { style, .. }) => style.width,
                    _ => return Task::none(),
                },
                PropertyTarget::NumberTool
                | PropertyTarget::TextTool
                | PropertyTarget::ShapeTool(_)
                | PropertyTarget::PixelateTool => return Task::none(),
            };
            let transaction = state
                .editor
                .properties
                .width
                .get_or_insert(StrokeWidthTransaction {
                    target,
                    original,
                    preview: original,
                });
            if transaction.target != target {
                *transaction = StrokeWidthTransaction {
                    target,
                    original,
                    preview: original,
                };
            }
            transaction.preview = width;
            state.editor.properties.color = None;
            state.editor.properties.opacity = None;
            state.editor.properties.popup = None;
            Task::none()
        }
        Message::ApplyStrokeWidth => {
            use super::properties::PropertyTarget;
            let Some(transaction) = state.editor.properties.width.take() else {
                return Task::none();
            };
            match transaction.target {
                PropertyTarget::TwoPointTool(TwoPointKind::Line) => {
                    state.annotation_defaults.values.line.width = transaction.preview;
                    persist_annotation_defaults(state);
                }
                PropertyTarget::TwoPointTool(TwoPointKind::Arrow) => {
                    state.annotation_defaults.values.arrow.width = transaction.preview;
                    persist_annotation_defaults(state);
                }
                PropertyTarget::FreehandTool(kind) => {
                    match kind {
                        rollshot_image_document::FreehandKind::Pen => {
                            state.annotation_defaults.values.pen.width = transaction.preview;
                        }
                        rollshot_image_document::FreehandKind::Highlighter => {
                            state.annotation_defaults.values.highlighter.width =
                                transaction.preview;
                        }
                    }
                    persist_annotation_defaults(state);
                }
                PropertyTarget::Annotation(id) => {
                    if let Some(
                        Annotation::TwoPoint { style, .. } | Annotation::Freehand { style, .. },
                    ) = state.document.image.annotation(id)
                    {
                        let mut new_style = *style;
                        new_style.width = transaction.preview;
                        if let Err(error) = state.document.image.set_stroke_style(id, new_style) {
                            state.message = Some(InlineMessage::Error(error.to_string()));
                        }
                    }
                }
                PropertyTarget::NumberTool
                | PropertyTarget::TextTool
                | PropertyTarget::ShapeTool(_)
                | PropertyTarget::PixelateTool => {}
            }
            Task::none()
        }
        Message::CancelStrokeWidth => {
            state.editor.properties.width = None;
            Task::none()
        }
        Message::PreviewStrokeOpacity(opacity) => {
            use super::properties::{OpacityTransaction, PropertyTarget};
            if !opacity.is_finite() {
                state.message = Some(InlineMessage::Error("opacity must be finite".into()));
                return Task::none();
            }
            let Some(target) = super::properties::property_target(state) else {
                return Task::none();
            };
            let original = match target {
                PropertyTarget::FreehandTool(
                    rollshot_image_document::FreehandKind::Highlighter,
                ) => state.annotation_defaults.values.highlighter.opacity,
                PropertyTarget::Annotation(id) => match state.document.image.annotation(id) {
                    Some(Annotation::Freehand {
                        kind: rollshot_image_document::FreehandKind::Highlighter,
                        style,
                        ..
                    }) => style.opacity,
                    _ => return Task::none(),
                },
                _ => return Task::none(),
            };
            let transaction = state
                .editor
                .properties
                .opacity
                .get_or_insert(OpacityTransaction {
                    target,
                    original,
                    preview: original,
                });
            if transaction.target != target {
                *transaction = OpacityTransaction {
                    target,
                    original,
                    preview: original,
                };
            }
            transaction.preview = opacity.clamp(0.1, 1.0);
            state.editor.properties.color = None;
            state.editor.properties.width = None;
            state.editor.properties.popup = None;
            Task::none()
        }
        Message::ApplyStrokeOpacity => {
            use super::properties::PropertyTarget;
            let Some(transaction) = state.editor.properties.opacity.take() else {
                return Task::none();
            };
            match transaction.target {
                PropertyTarget::FreehandTool(
                    rollshot_image_document::FreehandKind::Highlighter,
                ) => {
                    state.annotation_defaults.values.highlighter.opacity = transaction.preview;
                    persist_annotation_defaults(state);
                }
                PropertyTarget::Annotation(id) => {
                    if let Some(Annotation::Freehand { style, .. }) =
                        state.document.image.annotation(id)
                    {
                        let mut new_style = *style;
                        new_style.opacity = transaction.preview;
                        if let Err(error) = state.document.image.set_stroke_style(id, new_style) {
                            state.message = Some(InlineMessage::Error(error.to_string()));
                        }
                    }
                }
                _ => {}
            }
            Task::none()
        }
        Message::CancelStrokeOpacity => {
            state.editor.properties.opacity = None;
            Task::none()
        }
        Message::SetNumberSize(size) => {
            match super::properties::property_target(state) {
                Some(super::properties::PropertyTarget::NumberTool) => {
                    state.annotation_defaults.values.number.size = size;
                    persist_annotation_defaults(state);
                }
                Some(super::properties::PropertyTarget::Annotation(id)) => {
                    if let Some(Annotation::NumberCallout { style, .. }) =
                        state.document.image.annotation(id)
                    {
                        let mut next = *style;
                        next.size = size;
                        if let Err(error) = state.document.image.set_number_style(id, next) {
                            state.message = Some(InlineMessage::Error(error.to_string()));
                        }
                    }
                }
                _ => {}
            }
            Task::none()
        }
        Message::SetTextSize(size) => {
            match super::properties::property_target(state) {
                Some(super::properties::PropertyTarget::TextTool) => {
                    state.annotation_defaults.values.text.font_size = size;
                    persist_annotation_defaults(state);
                }
                Some(super::properties::PropertyTarget::Annotation(id)) => {
                    if let Some(Annotation::TextNote { style, .. }) =
                        state.document.image.annotation(id)
                    {
                        let mut next = *style;
                        next.font_size = size;
                        if let Err(error) = state.document.image.set_text_style(id, next) {
                            state.message = Some(InlineMessage::Error(error.to_string()));
                        }
                    }
                }
                _ => {}
            }
            Task::none()
        }
        Message::ToggleTextBackground => {
            match super::properties::property_target(state) {
                Some(super::properties::PropertyTarget::TextTool) => {
                    let background = &mut state.annotation_defaults.values.text.background;
                    *background = if background.is_some() {
                        None
                    } else {
                        rollshot_image_document::TextStyle::default().background
                    };
                    persist_annotation_defaults(state);
                }
                Some(super::properties::PropertyTarget::Annotation(id)) => {
                    if let Some(Annotation::TextNote { style, .. }) =
                        state.document.image.annotation(id)
                    {
                        let mut next = *style;
                        next.background = if next.background.is_some() {
                            None
                        } else {
                            rollshot_image_document::TextStyle::default().background
                        };
                        if let Err(error) = state.document.image.set_text_style(id, next) {
                            state.message = Some(InlineMessage::Error(error.to_string()));
                        }
                    }
                }
                _ => {}
            }
            Task::none()
        }
    }
}

fn persist_annotation_defaults(state: &mut super::ResultWorkspace) {
    let Some(path) = state.annotation_defaults.config_path.clone() else {
        return;
    };
    if let Err(error) =
        super::annotation_defaults::save_to(&path, &state.annotation_defaults.values)
    {
        if !state.annotation_defaults.warning_reported {
            state.annotation_defaults.warning_reported = true;
            state.message = Some(InlineMessage::Warning(format!(
                "Could not save annotation defaults: {error}"
            )));
        }
    }
}

/// Drop a selection whose annotation no longer exists (spec §15).
fn prune_stale_selection(state: &mut super::ResultWorkspace) {
    if let Some(id) = state.editor.selection {
        if state.document.image.annotation(id).is_none() {
            set_selection(state, None);
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
            if let Ok(id) =
                state
                    .document
                    .image
                    .add_text_note_with_style(draft.position, text, draft.style)
            {
                set_selection(state, Some(id));
            }
        }
        Some(id) => {
            let _ = state.document.image.set_text(id, text);
        }
    }
}

fn block_pending_candidates(state: &mut super::ResultWorkspace) -> bool {
    if let super::workbench::WorkspaceMode::Workbench(ref wb) = state.mode {
        if super::workbench::state::has_pending_candidates(wb) {
            state.message = Some(InlineMessage::Error(format!(
                "{}\nApply them before safe export.",
                super::workbench::state::apply_skip_summary(wb)
            )));
            return true;
        }
    }
    false
}

fn result_issue_pack_input(state: &super::ResultWorkspace) -> crate::issue_pack::IssuePackInput {
    let redaction_count = state
        .document
        .image
        .annotations()
        .iter()
        .filter(|annotation| matches!(annotation, Annotation::OpaqueRedaction { .. }))
        .count();
    crate::issue_pack::IssuePackInput {
        title: None,
        created_at: chrono::Local::now(),
        rollshot_version: env!("CARGO_PKG_VERSION").to_string(),
        platform: crate::issue_pack::PlatformInfo::current(),
        final_image: Some(crate::issue_pack::SafeImageAsset {
            file_name: "final-redacted.png".to_string(),
            pixels: state.document.image.flatten(),
            derived_from_original: true,
        }),
        action_guide: None,
        ocr_snippets: result_ocr_snippets(state),
        evidence_review: crate::issue_pack::EvidenceReviewSummary {
            required: true,
            completed: state
                .issue_pack
                .as_ref()
                .is_some_and(|dialog| dialog.review_confirmed),
            result_workspace_images_reviewed: state
                .issue_pack
                .as_ref()
                .is_some_and(|dialog| dialog.review_confirmed),
            action_guide_keyframes_reviewed: false,
        },
        redaction: crate::issue_pack::RedactionSummary {
            review_required: true,
            review_completed: state
                .issue_pack
                .as_ref()
                .is_some_and(|dialog| dialog.review_confirmed),
            result_workspace_images_are_flattened: true,
            original_pixels_included: false,
            redaction_count,
        },
        #[cfg(feature = "action-guide")]
        import_warnings: Vec::new(),
    }
}

#[cfg(feature = "ocr")]
fn result_ocr_snippets(state: &super::ResultWorkspace) -> Vec<crate::issue_pack::OcrSnippet> {
    state
        .ocr_text
        .document()
        .map(|document| {
            document
                .visible_items()
                .iter()
                .take(12)
                .map(|item| crate::issue_pack::OcrSnippet {
                    text: item.text.clone(),
                })
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(not(feature = "ocr"))]
fn result_ocr_snippets(_state: &super::ResultWorkspace) -> Vec<crate::issue_pack::OcrSnippet> {
    Vec::new()
}

fn begin_issue_pack_export(
    state: &mut super::ResultWorkspace,
    kind: super::IssuePackKind,
) -> Task<Message> {
    let Some(dialog) = &mut state.issue_pack else {
        return Task::none();
    };
    if !dialog.review_confirmed {
        state.message = Some(InlineMessage::Error(
            "Review the images included in this bug report before export.".to_string(),
        ));
        return Task::none();
    }
    dialog.pending_kind = Some(kind);
    let default_dir = crate::storage::Platform::current()
        .and_then(crate::storage::default_output_dir)
        .unwrap_or_else(|_| PathBuf::from("."));
    Task::perform(
        async move {
            rfd::AsyncFileDialog::new()
                .set_directory(default_dir)
                .pick_folder()
                .await
                .map(|h| h.path().to_path_buf())
        },
        Message::IssuePackFolderChosen,
    )
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
            "c" => Some(Message::KeyboardCopy),
            #[cfg(feature = "ocr")]
            "a" => Some(Message::SelectAllOcrText),
            _ => None,
        },
        keyboard::Key::Character(c) if !modifiers.alt() => match c.as_str() {
            "v" => Some(Message::SelectTool(Tool::Select)),
            "n" => Some(Message::SelectTool(Tool::Number)),
            "t" => Some(Message::SelectTool(Tool::Text)),
            "l" => Some(Message::SelectTool(Tool::Line)),
            "a" => Some(Message::SelectTool(Tool::Arrow)),
            "u" => Some(Message::SelectTool(Tool::Rectangle)),
            "o" if !cfg!(feature = "ocr") => Some(Message::SelectTool(Tool::Ellipse)),
            "r" => Some(Message::SelectTool(Tool::Redact)),
            "p" => Some(Message::SelectTool(Tool::Pen)),
            "h" => Some(Message::SelectTool(Tool::Highlighter)),
            "b" => Some(Message::SelectTool(Tool::Pixelate)),
            "s" => Some(Message::SelectRememberedShape),
            #[cfg(feature = "ocr")]
            "o" => Some(Message::SelectTool(Tool::OcrText)),
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
    use super::super::properties::preview_annotation;
    use super::super::two_point::bounded_constrained_endpoint;
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
        super::super::ResultWorkspace::with_config_path(
            super::super::document::ResultDocument::unsaved(image()),
            None,
            None,
        )
    }

    #[test]
    fn preview_failure_reports_one_inline_warning() {
        let mut state = workspace();
        let key = super::super::pixelate_preview::PreviewKey::new(
            1,
            rollshot_image_document::RasterRegion {
                x: 0,
                y: 0,
                width: 2,
                height: 2,
            },
            rollshot_image_document::DEFAULT_PIXELATE_BLOCK_SIZE,
            1.0,
        );
        let request = state.pixelate_previews.begin_request(key).unwrap();

        let _ = update_inner(
            &mut state,
            Message::PixelatePreviewReady(
                request,
                Err(super::super::pixelate_preview::PreviewGenerationError::WorkerFailed),
            ),
        );

        assert!(matches!(state.message, Some(InlineMessage::Warning(_))));
    }

    fn workspace_with_arrow() -> super::super::ResultWorkspace {
        let mut state = workspace();
        state
            .document
            .image
            .add_two_point(
                rollshot_image_document::TwoPointKind::Arrow,
                ImagePoint::new(0.0, 0.0),
                ImagePoint::new(1.0, 1.0),
            )
            .unwrap();
        state
    }

    #[test]
    fn width_preview_does_not_mutate_document_and_release_commits_once() {
        let mut state = workspace_with_arrow();
        let id = state.document.image.annotations()[0].id();
        state.editor.tool = Tool::Select;
        state.editor.selection = Some(id);
        let before = state.document.image.state_id();

        let _ = update(&mut state, Message::PreviewStrokeWidth(9.0));
        assert_eq!(state.document.image.state_id(), before);
        assert_eq!(
            preview_annotation(&state)
                .unwrap()
                .stroke_style()
                .unwrap()
                .width,
            9.0
        );

        let _ = update(&mut state, Message::ApplyStrokeWidth);
        assert_ne!(state.document.image.state_id(), before);
        assert_eq!(
            state
                .document
                .image
                .annotation(id)
                .unwrap()
                .stroke_style()
                .unwrap()
                .width,
            9.0
        );
        assert!(state.document.image.undo());
        assert_eq!(
            state
                .document
                .image
                .annotation(id)
                .unwrap()
                .stroke_style()
                .unwrap()
                .width,
            4.0
        );
    }

    #[test]
    fn selected_arrow_style_does_not_change_arrow_or_line_defaults() {
        let mut state = workspace_with_arrow();
        let defaults = state.annotation_defaults.values.clone();
        let id = state.document.image.annotations()[0].id();
        state.editor.tool = Tool::Select;
        state.editor.selection = Some(id);
        let _ = update(&mut state, Message::PreviewStrokeWidth(12.0));
        let _ = update(&mut state, Message::ApplyStrokeWidth);
        assert_eq!(state.annotation_defaults.values, defaults);
    }

    #[test]
    fn width_only_preview_cancel_and_target_change_never_mutate_document() {
        let mut state = workspace_with_arrow();
        let id = state.document.image.annotations()[0].id();
        state.editor.tool = Tool::Select;
        state.editor.selection = Some(id);
        let before = state.document.image.state_id();
        let _ = update(&mut state, Message::PreviewStrokeWidth(11.0));
        assert_eq!(
            preview_annotation(&state)
                .unwrap()
                .stroke_style()
                .unwrap()
                .width,
            11.0
        );
        let _ = update(&mut state, Message::CancelStrokeWidth);
        assert!(preview_annotation(&state).is_none());
        let _ = update(&mut state, Message::PreviewStrokeWidth(12.0));
        let _ = update(&mut state, Message::SelectTool(Tool::Arrow));
        assert!(preview_annotation(&state).is_none());
        assert_eq!(state.document.image.state_id(), before);
    }

    fn workspace_with_two_arrows() -> (super::super::ResultWorkspace, AnnotationId, AnnotationId) {
        let mut state = workspace_with_size(200, 200);
        let first = state
            .document
            .image
            .add_two_point(
                TwoPointKind::Arrow,
                ImagePoint::new(10.0, 20.0),
                ImagePoint::new(60.0, 20.0),
            )
            .unwrap();
        let second = state
            .document
            .image
            .add_two_point(
                TwoPointKind::Arrow,
                ImagePoint::new(100.0, 100.0),
                ImagePoint::new(150.0, 100.0),
            )
            .unwrap();
        state.editor.tool = Tool::Select;
        state.editor.selection = Some(first);
        (state, first, second)
    }

    #[test]
    fn selection_target_switches_clear_color_and_width_transactions() {
        use super::super::properties::ColorProperty;

        let (mut canvas_hit, _, second) = workspace_with_two_arrows();
        let _ = update(
            &mut canvas_hit,
            Message::OpenColorPicker(ColorProperty::StrokeColor),
        );
        let _ = update(
            &mut canvas_hit,
            Message::CanvasPressed(ImagePoint::new(150.0, 100.0)),
        );
        assert_eq!(canvas_hit.editor.selection, Some(second));
        assert!(canvas_hit.editor.properties.color.is_none());

        let (mut empty_canvas, _, _) = workspace_with_two_arrows();
        let _ = update(&mut empty_canvas, Message::PreviewStrokeWidth(9.0));
        let _ = update(
            &mut empty_canvas,
            Message::CanvasPressed(ImagePoint::new(190.0, 190.0)),
        );
        assert_eq!(empty_canvas.editor.selection, None);
        assert!(empty_canvas.editor.properties.width.is_none());

        let (mut navigator, _, second) = workspace_with_two_arrows();
        let _ = update(
            &mut navigator,
            Message::OpenColorPicker(ColorProperty::StrokeColor),
        );
        let _ = update(&mut navigator, Message::NavigatorJump(second));
        assert_eq!(navigator.editor.selection, Some(second));
        assert!(navigator.editor.properties.color.is_none());
    }

    fn unsaved_workspace() -> super::super::ResultWorkspace {
        super::super::ResultWorkspace::with_config_path(
            super::super::document::ResultDocument::unsaved(image()),
            None,
            None,
        )
    }

    fn saved_workspace() -> super::super::ResultWorkspace {
        super::super::ResultWorkspace::with_config_path(
            super::super::document::ResultDocument::saved(
                image(),
                std::path::PathBuf::from("/tmp/result.png"),
            ),
            None,
            None,
        )
    }

    #[cfg(feature = "ocr")]
    fn ocr_item(id: u64, text: &str, bounds: ImageRect) -> super::super::ocr_text::OcrTextItem {
        super::super::ocr_text::OcrTextItem {
            id: super::super::ocr_text::OcrItemId(id),
            text: text.into(),
            confidence: 0.95,
            bounds,
            quad: [
                ImagePoint::new(bounds.x, bounds.y),
                ImagePoint::new(bounds.x + bounds.width, bounds.y),
                ImagePoint::new(bounds.x + bounds.width, bounds.y + bounds.height),
                ImagePoint::new(bounds.x, bounds.y + bounds.height),
            ],
        }
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
    fn two_point_output_excludes_draft_and_handles() {
        let mut state = workspace_with_size(200, 200);
        state
            .document
            .image
            .add_two_point(
                TwoPointKind::Line,
                ImagePoint::new(20.0, 40.0),
                ImagePoint::new(120.0, 40.0),
            )
            .unwrap();
        let arrow_id = state
            .document
            .image
            .add_two_point(
                TwoPointKind::Arrow,
                ImagePoint::new(20.0, 100.0),
                ImagePoint::new(140.0, 100.0),
            )
            .unwrap();
        state.editor.selection = Some(arrow_id);
        let _ = update(&mut state, Message::SelectTool(Tool::Line));
        let _ = handle_canvas_pressed(&mut state, ImagePoint::new(20.0, 160.0), Instant::now());
        let _ = handle_canvas_moved(&mut state, ImagePoint::new(120.0, 160.0));

        assert!(matches!(
            state.editor.drag,
            Some(DragState::CreateTwoPoint { .. })
        ));
        assert_eq!(state.editor.selection, Some(arrow_id));

        let output = copy_payload(&state);
        assert_eq!(output, state.document.image.flatten());
        assert_ne!(
            output.get_pixel(60, 40),
            state.document.image.source().get_pixel(60, 40),
            "committed line must reach output"
        );
        assert_ne!(
            output.get_pixel(80, 100),
            state.document.image.source().get_pixel(80, 100),
            "committed arrow must reach output"
        );
        assert_eq!(
            output.get_pixel(80, 160),
            state.document.image.source().get_pixel(80, 160),
            "uncommitted draft must stay out of output"
        );
        assert_eq!(
            output.get_pixel(20, 95),
            state.document.image.source().get_pixel(20, 95),
            "selection handle must stay out of output"
        );

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

    #[cfg(feature = "ocr")]
    #[test]
    fn ocr_prepare_success_reports_visible_text_count() {
        let mut state = workspace();
        state.editor.tool = Tool::OcrText;

        let _ = update(
            &mut state,
            Message::OcrPrepared(Ok(vec![
                ocr_item(
                    0,
                    "hello",
                    ImageRect {
                        x: 0.0,
                        y: 0.0,
                        width: 1.0,
                        height: 1.0,
                    },
                ),
                ocr_item(
                    1,
                    "world",
                    ImageRect {
                        x: 1.0,
                        y: 0.0,
                        width: 1.0,
                        height: 1.0,
                    },
                ),
            ])),
        );

        assert_eq!(
            state.message_text().as_deref(),
            Some("OCR complete: 2 text blocks ready")
        );
    }

    #[cfg(feature = "ocr")]
    #[test]
    fn ocr_prepare_success_reports_when_no_visible_text_is_available() {
        let mut state = workspace();
        state.editor.tool = Tool::OcrText;

        let _ = update(&mut state, Message::OcrPrepared(Ok(Vec::new())));

        assert_eq!(
            state.message_text().as_deref(),
            Some("OCR complete: no visible text found")
        );
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
        let mut ws = super::super::ResultWorkspace::with_max_texture_dim(
            super::super::document::ResultDocument::unsaved(img),
            None,
            super::super::DEFAULT_MAX_TEXTURE_DIM,
        );
        ws.viewport.zoom = ZoomMode::ActualSize;
        ws.apply_viewport_bounds(Size::new(w as f32, h as f32));
        ws
    }

    fn endpoints(annotation: &Annotation) -> (ImagePoint, ImagePoint) {
        match annotation {
            Annotation::TwoPoint { start, end, .. } => (*start, *end),
            _ => panic!("expected TwoPoint annotation"),
        }
    }

    fn workspace_with_selected_arrow() -> super::super::ResultWorkspace {
        let mut state = workspace_with_size(200, 200);
        let id = state
            .document
            .image
            .add_two_point(
                TwoPointKind::Arrow,
                ImagePoint::new(10.0, 20.0),
                ImagePoint::new(80.0, 40.0),
            )
            .unwrap();
        state.editor.tool = Tool::Select;
        state.editor.selection = Some(id);
        state
    }

    fn current_drag_annotation(state: &super::super::ResultWorkspace) -> Option<Annotation> {
        match &state.editor.drag {
            Some(DragState::CreateTwoPoint {
                kind,
                start,
                raw_current,
                style,
            }) => Some(Annotation::two_point_with_style(
                AnnotationId(u64::MAX),
                *kind,
                *start,
                bounded_constrained_endpoint(
                    *start,
                    *raw_current,
                    state.modifiers.shift(),
                    state.document.image.source().width(),
                    state.document.image.source().height(),
                ),
                *style,
            )),
            Some(DragState::EditAnnotation { current, .. }) => Some(current.clone()),
            _ => None,
        }
    }

    #[test]
    fn arrow_creation_previews_and_commits_same_snapped_endpoint_without_selection() {
        let mut state = workspace_with_size(200, 200);
        state.editor.tool = Tool::Arrow;
        let _ = update(
            &mut state,
            Message::ModifiersChanged(iced::keyboard::Modifiers::SHIFT),
        );
        let _ = handle_canvas_pressed(&mut state, ImagePoint::new(10.0, 10.0), Instant::now());
        let _ = handle_canvas_moved(&mut state, ImagePoint::new(90.0, 36.0));
        let preview = current_drag_annotation(&state).unwrap();
        let _ = handle_canvas_released(&mut state, ImagePoint::new(90.0, 36.0));
        let committed = &state.document.image.annotations()[0];
        assert_eq!(endpoints(committed), endpoints(&preview));
        assert_eq!(committed.stroke_style(), preview.stroke_style());
        assert_eq!(state.editor.tool, Tool::Arrow);
        assert_eq!(state.editor.selection, None);
    }

    #[test]
    fn shifted_creation_preview_and_commit_share_bounded_endpoint() {
        let mut state = workspace_with_size(100, 100);
        state.editor.tool = Tool::Arrow;
        let _ = update(
            &mut state,
            Message::ModifiersChanged(iced::keyboard::Modifiers::SHIFT),
        );
        let start = ImagePoint::new(90.0, 50.0);
        let pointer = ImagePoint::new(100.0, 70.0);
        let _ = handle_canvas_pressed(&mut state, start, Instant::now());
        let _ = handle_canvas_moved(&mut state, pointer);

        let preview = current_drag_annotation(&state).unwrap();
        let (_, preview_end) = endpoints(&preview);
        assert_eq!(preview_end, ImagePoint::new(100.0, 60.0));

        let _ = handle_canvas_released(&mut state, pointer);
        let committed = &state.document.image.annotations()[0];
        assert_eq!(endpoints(committed), endpoints(&preview));
    }

    #[test]
    fn shifted_endpoint_preview_and_commit_share_bounded_endpoint() {
        let mut state = workspace_with_size(100, 100);
        let id = state
            .document
            .image
            .add_two_point(
                TwoPointKind::Arrow,
                ImagePoint::new(90.0, 50.0),
                ImagePoint::new(90.0, 80.0),
            )
            .unwrap();
        state.editor.tool = Tool::Select;
        state.editor.selection = Some(id);
        let _ = update(
            &mut state,
            Message::ModifiersChanged(iced::keyboard::Modifiers::SHIFT),
        );
        let pointer = ImagePoint::new(100.0, 70.0);
        let _ = handle_canvas_pressed(&mut state, ImagePoint::new(90.0, 80.0), Instant::now());
        let _ = handle_canvas_moved(&mut state, pointer);

        let preview = current_drag_annotation(&state).unwrap();
        assert_eq!(
            endpoints(&preview),
            (ImagePoint::new(90.0, 50.0), ImagePoint::new(100.0, 60.0))
        );

        let _ = handle_canvas_released(&mut state, pointer);
        assert_eq!(
            endpoints(state.document.image.annotation(id).unwrap()),
            endpoints(&preview)
        );
    }

    #[test]
    fn body_move_preview_and_commit_preserve_vector_at_image_edge() {
        let mut state = workspace_with_size(100, 100);
        let id = state
            .document
            .image
            .add_two_point(
                TwoPointKind::Arrow,
                ImagePoint::new(60.0, 60.0),
                ImagePoint::new(90.0, 80.0),
            )
            .unwrap();
        state.editor.tool = Tool::Select;
        state.editor.selection = Some(id);
        let press = ImagePoint::new(75.0, 70.0);
        let release = ImagePoint::new(100.0, 100.0);
        let _ = handle_canvas_pressed(&mut state, press, Instant::now());
        let _ = handle_canvas_moved(&mut state, release);

        let preview = current_drag_annotation(&state).unwrap();
        assert_eq!(
            endpoints(&preview),
            (ImagePoint::new(70.0, 80.0), ImagePoint::new(100.0, 100.0))
        );

        let _ = handle_canvas_released(&mut state, release);
        assert_eq!(
            endpoints(state.document.image.annotation(id).unwrap()),
            endpoints(&preview)
        );
    }

    #[test]
    fn two_point_creation_captures_style_at_press_and_adds_one_history_entry() {
        let mut state = workspace_with_size(200, 200);
        state.editor.tool = Tool::Line;
        let pressed_style = state.annotation_defaults.values.line;
        let _ = handle_canvas_pressed(&mut state, ImagePoint::new(10.0, 10.0), Instant::now());
        state.annotation_defaults.values.line.width = pressed_style.width + 5.0;
        let _ = handle_canvas_released(&mut state, ImagePoint::new(70.0, 10.0));

        assert_eq!(
            state.document.image.annotations()[0].stroke_style(),
            Some(pressed_style)
        );
        assert!(state.document.image.undo());
        assert!(state.document.image.annotations().is_empty());
        assert!(!state.document.image.can_undo());
    }

    #[test]
    fn sub_threshold_two_point_gesture_creates_no_history() {
        let mut state = workspace_with_size(200, 200);
        state.editor.tool = Tool::Line;
        let _ = handle_canvas_pressed(&mut state, ImagePoint::new(10.0, 10.0), Instant::now());
        let _ = handle_canvas_released(&mut state, ImagePoint::new(11.0, 10.0));
        assert!(state.document.image.annotations().is_empty());
        assert!(!state.document.image.can_undo());
    }

    #[test]
    fn shift_toggle_recomputes_endpoint_preview_before_pointer_moves_again() {
        let mut state = workspace_with_selected_arrow();
        let id = state.editor.selection.unwrap();
        let (_, end) = endpoints(state.document.image.annotation(id).unwrap());
        let _ = handle_canvas_pressed(&mut state, end, Instant::now());
        let _ = handle_canvas_moved(&mut state, ImagePoint::new(90.0, 36.0));
        let unsnapped = current_drag_annotation(&state).unwrap();
        let _ = update(
            &mut state,
            Message::ModifiersChanged(iced::keyboard::Modifiers::SHIFT),
        );
        let snapped = current_drag_annotation(&state).unwrap();
        assert_ne!(snapped, unsnapped);
    }

    #[test]
    fn endpoint_drag_fixes_the_opposite_endpoint_and_commits_one_edit() {
        let mut state = workspace_with_selected_arrow();
        let id = state.editor.selection.unwrap();
        let original = state.document.image.annotation(id).unwrap().clone();
        let (start, end) = endpoints(&original);
        let _ = handle_canvas_pressed(&mut state, end, Instant::now());
        let _ = handle_canvas_released(&mut state, ImagePoint::new(120.0, 70.0));
        assert_eq!(
            endpoints(state.document.image.annotation(id).unwrap()),
            (start, ImagePoint::new(120.0, 70.0))
        );
        assert!(state.document.image.undo());
        assert_eq!(state.document.image.annotation(id), Some(&original));
    }

    #[test]
    fn release_point_is_used_even_without_a_final_move_event() {
        let mut state = workspace_with_size(200, 200);
        state.editor.tool = Tool::Line;
        let _ = handle_canvas_pressed(&mut state, ImagePoint::new(10.0, 10.0), Instant::now());
        let _ = handle_canvas_moved(&mut state, ImagePoint::new(30.0, 10.0));
        let _ = handle_canvas_released(&mut state, ImagePoint::new(70.0, 10.0));
        assert_eq!(
            endpoints(&state.document.image.annotations()[0]),
            (ImagePoint::new(10.0, 10.0), ImagePoint::new(70.0, 10.0))
        );
    }

    #[test]
    fn escape_cancels_two_point_creation_without_history() {
        let mut state = workspace_with_size(200, 200);
        state.editor.tool = Tool::Arrow;
        let _ = handle_canvas_pressed(&mut state, ImagePoint::new(10.0, 10.0), Instant::now());
        let _ = handle_canvas_moved(&mut state, ImagePoint::new(80.0, 40.0));
        let _ = update(&mut state, Message::EscapePressed);
        assert!(state.editor.drag.is_none());
        assert!(state.document.image.annotations().is_empty());
        assert!(!state.document.image.can_undo());
        assert_eq!(state.editor.tool, Tool::Arrow);
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
        assert_eq!(
            map_key_press(&Key::Character("n".into()), none, false),
            Some(Message::SelectTool(Tool::Number))
        );
        assert_eq!(
            map_key_press(&Key::Character("z".into()), zmod(), false),
            Some(Message::Undo)
        );
        assert_eq!(
            map_key_press(
                &Key::Character("z".into()),
                zmod() | keyboard::Modifiers::SHIFT,
                false,
            ),
            Some(Message::Redo)
        );
        assert_eq!(
            map_key_press(&Key::Named(Named::Delete), none, false),
            Some(Message::DeleteSelected)
        );
        assert_eq!(
            map_key_press(&Key::Character("c".into()), zmod(), false),
            Some(Message::KeyboardCopy)
        );
    }

    #[test]
    fn captured_keys_are_ignored_except_escape() {
        use keyboard::{key::Named, Key};
        let none = keyboard::Modifiers::default();
        assert_eq!(map_key_press(&Key::Character("n".into()), none, true), None);
        assert_eq!(
            map_key_press(&Key::Named(Named::Backspace), none, true),
            None
        );
        assert_eq!(
            map_key_press(&Key::Named(Named::Escape), none, true),
            Some(Message::EscapePressed)
        );
    }

    #[test]
    fn map_key_press_routes_line_and_arrow_when_input_is_not_captured() {
        let modifiers = keyboard::Modifiers::empty();
        assert_eq!(
            map_key_press(&keyboard::Key::Character("l".into()), modifiers, false),
            Some(Message::SelectTool(Tool::Line))
        );
        assert_eq!(
            map_key_press(&keyboard::Key::Character("a".into()), modifiers, false),
            Some(Message::SelectTool(Tool::Arrow))
        );
    }

    #[test]
    fn map_key_press_captured_input_blocks_two_point_shortcuts() {
        let modifiers = keyboard::Modifiers::empty();
        assert_eq!(
            map_key_press(&keyboard::Key::Character("l".into()), modifiers, true),
            None
        );
        assert_eq!(
            map_key_press(&keyboard::Key::Character("a".into()), modifiers, true),
            None
        );
    }

    #[test]
    fn plain_characters_do_not_fire_with_command_modifiers_held() {
        use keyboard::Key;
        assert_eq!(
            map_key_press(&Key::Character("n".into()), zmod(), false),
            None
        );
    }

    // -- Pen & Highlighter shortcuts (Task 5) ------------------------------

    #[test]
    fn p_and_h_select_freehand_tools() {
        let p = keyboard::Key::Character("p".into());
        let h = keyboard::Key::Character("h".into());
        assert_eq!(
            map_key_press(&p, keyboard::Modifiers::default(), false),
            Some(Message::SelectTool(Tool::Pen))
        );
        assert_eq!(
            map_key_press(&h, keyboard::Modifiers::default(), false),
            Some(Message::SelectTool(Tool::Highlighter))
        );
        // Captured input ignores tool shortcuts.
        assert_eq!(
            map_key_press(&p, keyboard::Modifiers::default(), true),
            None
        );
        assert_eq!(
            map_key_press(&h, keyboard::Modifiers::default(), true),
            None
        );
    }

    #[test]
    fn command_modified_p_and_h_yield_to_command_handlers() {
        // Command+P / Command+H should NOT trigger tool selection (they go to native handlers).
        assert_eq!(
            map_key_press(&keyboard::Key::Character("p".into()), zmod(), false),
            None
        );
        assert_eq!(
            map_key_press(&keyboard::Key::Character("h".into()), zmod(), false),
            None
        );
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

    #[cfg(feature = "ocr")]
    #[test]
    fn copy_without_ocr_selection_shows_error() {
        let mut state = workspace();
        state.editor.tool = Tool::OcrText;
        state.ocr_text.set_ready_for_tests(vec![]);

        let _ = update(&mut state, Message::CopyOcrSelection);

        assert_eq!(
            state.message.as_ref().map(InlineMessage::text),
            Some("No OCR text selected")
        );
    }

    #[cfg(feature = "ocr")]
    #[test]
    fn toolbar_copy_in_ocr_mode_still_uses_image_copy_path() {
        let mut state = workspace();
        state.editor.tool = Tool::OcrText;
        state.ocr_text.set_ready_for_tests(vec![]);

        let _ = update(&mut state, Message::Copy);

        assert_ne!(
            state.message.as_ref().map(InlineMessage::text),
            Some("No OCR text selected")
        );
    }

    #[cfg(feature = "ocr")]
    #[test]
    fn command_c_maps_to_keyboard_copy() {
        let msg = map_key_press(&keyboard::Key::Character("c".into()), zmod(), false);

        assert_eq!(msg, Some(Message::KeyboardCopy));
    }

    #[cfg(feature = "ocr")]
    #[test]
    fn keyboard_copy_in_ocr_mode_uses_ocr_selection() {
        let mut state = workspace();
        state.editor.tool = Tool::OcrText;
        state.ocr_text.set_ready_for_tests(vec![]);

        let _ = update(&mut state, Message::KeyboardCopy);

        assert_eq!(
            state.message.as_ref().map(InlineMessage::text),
            Some("No OCR text selected")
        );
    }

    // -- OCR text mode (Task 4) -----------------------------------------------

    #[cfg(feature = "ocr")]
    #[test]
    fn selecting_ocr_tool_clears_annotation_drag_and_requests_prepare() {
        let mut state = workspace();
        state.editor.drag = Some(DragState::Pan {
            last_pointer: Point::new(10.0, 10.0),
        });

        let _ = update(&mut state, Message::SelectTool(Tool::OcrText));

        assert_eq!(state.editor.tool, Tool::OcrText);
        assert!(state.editor.drag.is_none());
        assert!(state.ocr_text.is_preparing_or_ready());
    }

    #[cfg(feature = "ocr")]
    #[test]
    fn canvas_press_in_ocr_text_mode_does_not_start_annotation_drag() {
        let mut state = workspace();
        state.editor.tool = Tool::OcrText;
        state
            .ocr_text
            .set_ready_for_tests(vec![crate::result_workspace::ocr_text::OcrTextItem {
                id: crate::result_workspace::ocr_text::OcrItemId(0),
                text: "secret".into(),
                confidence: 0.95,
                bounds: ImageRect {
                    x: 10.0,
                    y: 10.0,
                    width: 80.0,
                    height: 18.0,
                },
                quad: [
                    ImagePoint { x: 10.0, y: 10.0 },
                    ImagePoint { x: 90.0, y: 10.0 },
                    ImagePoint { x: 90.0, y: 28.0 },
                    ImagePoint { x: 10.0, y: 28.0 },
                ],
            }]);

        let _ = handle_canvas_pressed(&mut state, ImagePoint::new(12.0, 12.0), Instant::now());

        assert!(state.editor.drag.is_none());
        assert!(state.ocr_text.selection().is_none());
    }

    #[cfg(feature = "ocr")]
    #[test]
    fn keyboard_copy_in_ocr_mode_routes_to_copy_ocr_selection() {
        let mut state = workspace();
        state.editor.tool = Tool::OcrText;
        state
            .ocr_text
            .set_ready_for_tests(vec![crate::result_workspace::ocr_text::OcrTextItem {
                id: crate::result_workspace::ocr_text::OcrItemId(0),
                text: "secret".into(),
                confidence: 0.95,
                bounds: ImageRect {
                    x: 10.0,
                    y: 10.0,
                    width: 80.0,
                    height: 18.0,
                },
                quad: [
                    ImagePoint { x: 10.0, y: 10.0 },
                    ImagePoint { x: 90.0, y: 10.0 },
                    ImagePoint { x: 90.0, y: 28.0 },
                    ImagePoint { x: 10.0, y: 28.0 },
                ],
            }]);
        state.ocr_text.set_selection(Some(
            crate::result_workspace::ocr_text::OcrSelection::range(
                crate::result_workspace::ocr_text::TextCursor::new(0, 0),
                crate::result_workspace::ocr_text::TextCursor::new(0, 6),
            ),
        ));

        let _task = update(&mut state, Message::KeyboardCopy);
        assert!(
            state.message.is_none(),
            "keyboard copy in OCR mode with valid selection does not set an error"
        );

        let _task = update(&mut state, Message::CopyOcrFinished(Ok(())));
        assert_eq!(
            state.message.as_ref().map(InlineMessage::text),
            Some("Copied OCR text"),
            "CopyOcrFinished(Ok) shows success message"
        );
    }

    #[cfg(feature = "ocr")]
    #[test]
    fn tool_switching_is_blocked_in_ocr_mode() {
        let mut state = workspace();
        state.editor.tool = Tool::OcrText;
        state
            .ocr_text
            .set_ready_for_tests(vec![crate::result_workspace::ocr_text::OcrTextItem {
                id: crate::result_workspace::ocr_text::OcrItemId(0),
                text: "secret".into(),
                confidence: 0.95,
                bounds: ImageRect {
                    x: 10.0,
                    y: 10.0,
                    width: 80.0,
                    height: 18.0,
                },
                quad: [
                    ImagePoint { x: 10.0, y: 10.0 },
                    ImagePoint { x: 90.0, y: 10.0 },
                    ImagePoint { x: 90.0, y: 28.0 },
                    ImagePoint { x: 10.0, y: 28.0 },
                ],
            }]);

        let _ = update(&mut state, Message::SelectTool(Tool::Select));
        assert_eq!(state.editor.tool, Tool::OcrText, "tool switch blocked");

        let _ = update(&mut state, Message::SelectTool(Tool::Number));
        assert_eq!(state.editor.tool, Tool::OcrText, "tool switch blocked");

        let _ = update(&mut state, Message::SelectTool(Tool::OcrText));
        assert_eq!(
            state.editor.tool,
            Tool::OcrText,
            "OcrText → OcrText is a no-op"
        );
    }

    #[cfg(feature = "ocr")]
    #[test]
    fn undo_redo_are_blocked_in_ocr_mode() {
        let mut state = workspace();
        state.editor.tool = Tool::OcrText;
        state
            .ocr_text
            .set_ready_for_tests(vec![crate::result_workspace::ocr_text::OcrTextItem {
                id: crate::result_workspace::ocr_text::OcrItemId(0),
                text: "secret".into(),
                confidence: 0.95,
                bounds: ImageRect {
                    x: 10.0,
                    y: 10.0,
                    width: 80.0,
                    height: 18.0,
                },
                quad: [
                    ImagePoint { x: 10.0, y: 10.0 },
                    ImagePoint { x: 90.0, y: 10.0 },
                    ImagePoint { x: 90.0, y: 28.0 },
                    ImagePoint { x: 10.0, y: 28.0 },
                ],
            }]);
        state
            .document
            .image
            .add_number_callout(ImagePoint::new(1.0, 1.0), ImagePoint::new(1.0, 1.0));
        let before = state.document.image.state_id();

        let _ = update(&mut state, Message::Undo);
        assert_eq!(
            state.document.image.state_id(),
            before,
            "undo blocked in OCR mode"
        );

        let _ = update(&mut state, Message::Redo);
        assert_eq!(
            state.document.image.state_id(),
            before,
            "redo blocked in OCR mode"
        );
    }

    #[cfg(feature = "ocr")]
    #[test]
    fn escape_clears_ocr_selection_before_leaving_ocr_mode() {
        let mut state = workspace();
        state.editor.tool = Tool::OcrText;
        state
            .ocr_text
            .set_ready_for_tests(vec![crate::result_workspace::ocr_text::OcrTextItem {
                id: crate::result_workspace::ocr_text::OcrItemId(0),
                text: "secret".into(),
                confidence: 0.95,
                bounds: ImageRect {
                    x: 10.0,
                    y: 10.0,
                    width: 80.0,
                    height: 18.0,
                },
                quad: [
                    ImagePoint { x: 10.0, y: 10.0 },
                    ImagePoint { x: 90.0, y: 10.0 },
                    ImagePoint { x: 90.0, y: 28.0 },
                    ImagePoint { x: 10.0, y: 28.0 },
                ],
            }]);
        state.ocr_text.set_selection(Some(
            crate::result_workspace::ocr_text::OcrSelection::range(
                crate::result_workspace::ocr_text::TextCursor::new(0, 0),
                crate::result_workspace::ocr_text::TextCursor::new(0, 3),
            ),
        ));

        let _ = update(&mut state, Message::EscapePressed);
        assert_eq!(state.editor.tool, Tool::OcrText);
        assert!(state.ocr_text.selection().is_none());

        let _ = update(&mut state, Message::EscapePressed);
        assert_eq!(state.editor.tool, Tool::Select);
    }

    // -- issue pack export (Task 5) ------------------------------------------

    #[test]
    fn issue_pack_request_blocks_pending_smart_redaction_candidates() {
        let mut state = workspace();
        state.mode = super::super::workbench::WorkspaceMode::Workbench(
            super::super::workbench::state::workbench_with_pending_candidate(),
        );

        let _ = update(&mut state, Message::ExportBugReport);

        assert!(state.issue_pack.is_none());
        assert!(state.message.as_ref().unwrap().text().contains("Apply"));
    }

    #[test]
    fn issue_pack_review_redactions_from_workbench_returns_to_normal_redact_mode() {
        let mut state = workspace();
        state.mode = super::super::workbench::WorkspaceMode::Workbench(
            super::super::workbench::WorkbenchState::default(),
        );
        let _ = update(&mut state, Message::ExportBugReport);

        let _ = update(&mut state, Message::IssuePackReviewRedactions);

        assert!(state.issue_pack.is_none());
        assert_eq!(state.editor.tool, Tool::Redact);
        assert!(matches!(
            state.mode,
            super::super::workbench::WorkspaceMode::Normal
        ));
    }

    #[test]
    fn issue_pack_export_requires_review_confirmation() {
        let mut state = workspace();
        let _ = update(&mut state, Message::ExportBugReport);
        assert!(state
            .issue_pack
            .as_ref()
            .is_some_and(|dialog| !dialog.review_confirmed));

        let tmp = tempfile::tempdir().unwrap();
        let _ = update(
            &mut state,
            Message::IssuePackFolderChosen(Some(tmp.path().to_path_buf())),
        );

        assert!(std::fs::read_dir(tmp.path()).unwrap().next().is_none());
        assert!(state.message.as_ref().unwrap().text().contains("review"));
    }

    #[test]
    fn issue_pack_folder_export_writes_flattened_result_image() {
        let mut state = workspace();
        state
            .document
            .image
            .add_redaction(ImageRect {
                x: 0.0,
                y: 0.0,
                width: 2.0,
                height: 2.0,
            })
            .unwrap();
        let tmp = tempfile::tempdir().unwrap();

        let _ = update(&mut state, Message::ExportBugReport);
        let _ = update(&mut state, Message::IssuePackReviewChanged(true));
        let _ = update(
            &mut state,
            Message::IssuePackFolderChosen(Some(tmp.path().to_path_buf())),
        );

        let final_image = tmp
            .path()
            .join("rollshot-issue-pack-")
            .parent()
            .unwrap()
            .read_dir()
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .find(|path| {
                path.file_name()
                    .unwrap()
                    .to_string_lossy()
                    .starts_with("rollshot-issue-pack-")
            })
            .unwrap()
            .join("images/final-redacted.png");
        let decoded = image::open(final_image).unwrap().to_rgba8();
        assert_eq!(decoded.get_pixel(0, 0).0, [0, 0, 0, 255]);
    }

    // -- property editing tests (Task 4) ------------------------------------

    fn workspace_with_selected_number() -> super::super::ResultWorkspace {
        let mut state = workspace_with_size(200, 200);
        let id = state
            .document
            .image
            .add_number_callout(ImagePoint::new(10.0, 10.0), ImagePoint::new(10.0, 10.0));
        state.editor.tool = Tool::Select;
        state.editor.selection = Some(id);
        state
    }

    fn workspace_with_selected_text() -> super::super::ResultWorkspace {
        let mut state = workspace_with_size(200, 200);
        let id = state
            .document
            .image
            .add_text_note(ImagePoint::new(10.0, 10.0), "note".into())
            .unwrap();
        state.editor.tool = Tool::Select;
        state.editor.selection = Some(id);
        state
    }

    #[test]
    fn open_color_picker_snapshots_original_color() {
        let mut state = workspace_with_selected_number();
        let _ = update(
            &mut state,
            Message::OpenColorPicker(super::super::properties::ColorProperty::NumberAccent),
        );
        let tx = state.editor.properties.color.as_ref().unwrap();
        assert_eq!(tx.original, tx.preview);
        assert_eq!(
            tx.original,
            rollshot_image_document::NumberStyle::default().accent
        );
        assert_eq!(
            tx.hex,
            format!(
                "#{:02X}{:02X}{:02X}",
                tx.original.r, tx.original.g, tx.original.b
            )
        );
    }

    #[test]
    fn preview_color_updates_preview_without_mutation() {
        let mut state = workspace_with_selected_number();
        let before = state.document.image.state_id();
        let _ = update(
            &mut state,
            Message::OpenColorPicker(super::super::properties::ColorProperty::NumberAccent),
        );
        let _ = update(&mut state, Message::PreviewColor(Rgb8::new(1, 2, 3)));
        let tx = state.editor.properties.color.as_ref().unwrap();
        assert_eq!(tx.preview, Rgb8::new(1, 2, 3));
        assert_eq!(
            state.document.image.state_id(),
            before,
            "no document mutation"
        );
    }

    #[test]
    fn cancel_color_restores_preview_without_history_or_default_save() {
        let mut state = workspace_with_selected_number();
        let before = state.document.image.state_id();
        let _ = update(
            &mut state,
            Message::OpenColorPicker(super::super::properties::ColorProperty::NumberAccent),
        );
        let _ = update(&mut state, Message::PreviewColor(Rgb8::new(1, 2, 3)));
        let _ = update(&mut state, Message::CancelColor);
        assert_eq!(state.document.image.state_id(), before);
        assert!(state.editor.properties.color.is_none());
    }

    #[test]
    fn apply_color_to_annotation_mutates_document() {
        let mut state = workspace_with_selected_number();
        let before = state.document.image.state_id();
        let _ = update(
            &mut state,
            Message::OpenColorPicker(super::super::properties::ColorProperty::NumberAccent),
        );
        let _ = update(&mut state, Message::PreviewColor(Rgb8::new(10, 20, 30)));
        let _ = update(&mut state, Message::ApplyColor);
        assert_ne!(state.document.image.state_id(), before, "document mutated");
        assert!(state.editor.properties.color.is_none());
        let id = state.editor.selection.unwrap();
        match state.document.image.annotation(id).unwrap() {
            Annotation::NumberCallout { style, .. } => {
                assert_eq!(style.accent, Rgb8::new(10, 20, 30));
            }
            _ => panic!("expected NumberCallout"),
        }
    }

    #[test]
    fn apply_color_to_text_annotation_mutates_document() {
        let mut state = workspace_with_selected_text();
        let before = state.document.image.state_id();
        let _ = update(
            &mut state,
            Message::OpenColorPicker(super::super::properties::ColorProperty::TextColor),
        );
        let _ = update(&mut state, Message::PreviewColor(Rgb8::new(40, 50, 60)));
        let _ = update(&mut state, Message::ApplyColor);
        assert_ne!(state.document.image.state_id(), before);
        let id = state.editor.selection.unwrap();
        match state.document.image.annotation(id).unwrap() {
            Annotation::TextNote { style, .. } => {
                assert_eq!(style.text_color, Rgb8::new(40, 50, 60));
            }
            _ => panic!("expected TextNote"),
        }
    }

    #[test]
    fn color_hex_changed_updates_hex_and_preview() {
        let mut state = workspace_with_selected_number();
        let _ = update(
            &mut state,
            Message::OpenColorPicker(super::super::properties::ColorProperty::NumberAccent),
        );
        let _ = update(&mut state, Message::ColorHexChanged("#AABBCC".into()));
        let tx = state.editor.properties.color.as_ref().unwrap();
        assert_eq!(tx.hex, "#AABBCC");
        assert_eq!(tx.preview, Rgb8::new(0xAA, 0xBB, 0xCC));
    }

    #[test]
    fn color_hex_changed_with_invalid_does_not_update_preview() {
        let mut state = workspace_with_selected_number();
        let _ = update(
            &mut state,
            Message::OpenColorPicker(super::super::properties::ColorProperty::NumberAccent),
        );
        let original = state.editor.properties.color.as_ref().unwrap().preview;
        let _ = update(&mut state, Message::ColorHexChanged("ZZZZZZ".into()));
        let tx = state.editor.properties.color.as_ref().unwrap();
        assert_eq!(
            tx.preview, original,
            "invalid hex preserves previous preview"
        );
    }

    #[test]
    fn commit_next_number_parses_and_sets() {
        let mut state = workspace_with_size(200, 200);
        state.editor.tool = Tool::Number;
        state.editor.properties.next_number_input = "42".into();
        let _ = update(&mut state, Message::CommitNextNumber);
        assert_eq!(state.document.image.next_number(), 42);
        assert!(state.editor.properties.next_number_input.is_empty());
    }

    #[test]
    fn commit_next_number_rejects_zero() {
        let mut state = workspace_with_size(200, 200);
        state.editor.tool = Tool::Number;
        state.editor.properties.next_number_input = "0".into();
        let _ = update(&mut state, Message::CommitNextNumber);
        assert!(state.message.is_some());
        assert!(state.message.as_ref().unwrap().is_error());
    }

    #[test]
    fn commit_next_number_rejects_non_numeric() {
        let mut state = workspace_with_size(200, 200);
        state.editor.tool = Tool::Number;
        state.editor.properties.next_number_input = "abc".into();
        let _ = update(&mut state, Message::CommitNextNumber);
        assert!(state.message.is_some());
        assert!(state.message.as_ref().unwrap().is_error());
    }

    #[test]
    fn open_color_picker_on_redact_tool_is_noop() {
        let mut state = workspace_with_size(200, 200);
        state.editor.tool = Tool::Redact;
        let _ = update(
            &mut state,
            Message::OpenColorPicker(super::super::properties::ColorProperty::NumberAccent),
        );
        assert!(state.editor.properties.color.is_none());
    }

    #[test]
    fn apply_color_on_no_transaction_is_noop() {
        let mut state = workspace_with_size(200, 200);
        let before = state.document.image.state_id();
        let _ = update(&mut state, Message::ApplyColor);
        assert_eq!(state.document.image.state_id(), before);
    }

    #[test]
    fn discrete_size_edits_update_tool_defaults_and_selected_annotations() {
        let mut defaults = workspace_with_size(200, 200);
        defaults.editor.tool = Tool::Number;
        let _ = update(
            &mut defaults,
            Message::SetNumberSize(rollshot_image_document::NumberSize::Large),
        );
        assert_eq!(
            defaults.annotation_defaults.values.number.size,
            rollshot_image_document::NumberSize::Large
        );

        let mut selected = workspace_with_selected_text();
        let before = selected.document.image.state_id();
        let _ = update(
            &mut selected,
            Message::SetTextSize(rollshot_image_document::TextSize::Px32),
        );
        assert_ne!(selected.document.image.state_id(), before);
        let id = selected.editor.selection.unwrap();
        assert!(matches!(
            selected.document.image.annotation(id),
            Some(Annotation::TextNote { style, .. })
                if style.font_size == rollshot_image_document::TextSize::Px32
        ));
    }

    #[test]
    fn background_toggle_updates_defaults_and_selected_annotations() {
        let mut defaults = workspace_with_size(200, 200);
        defaults.editor.tool = Tool::Text;
        let _ = update(&mut defaults, Message::ToggleTextBackground);
        assert_eq!(defaults.annotation_defaults.values.text.background, None);

        let mut selected = workspace_with_selected_text();
        let _ = update(&mut selected, Message::ToggleTextBackground);
        let id = selected.editor.selection.unwrap();
        assert!(matches!(
            selected.document.image.annotation(id),
            Some(Annotation::TextNote { style, .. }) if style.background.is_none()
        ));
    }

    #[test]
    fn opening_a_popup_closes_the_previous_popup() {
        use super::super::properties::{ColorProperty, Popup};

        let mut state = workspace_with_selected_number();
        let _ = update(&mut state, Message::ToggleCopyMenu);
        assert_eq!(state.editor.properties.popup, Some(Popup::CopyMenu));

        let _ = update(&mut state, Message::ToggleMoreMenu);
        assert!(!state.editor.copy_menu_open);
        assert_eq!(state.editor.properties.popup, Some(Popup::MoreMenu));

        let _ = update(&mut state, Message::ToggleNavigator);
        assert!(!state.editor.more_menu_open);
        assert_eq!(state.editor.properties.popup, None);

        let _ = update(&mut state, Message::ToggleMoreMenu);

        let _ = update(
            &mut state,
            Message::OpenColorPicker(ColorProperty::NumberAccent),
        );
        assert!(!state.editor.more_menu_open);
        assert_eq!(state.editor.properties.popup, Some(Popup::ColorPicker));

        let _ = update(&mut state, Message::EscapePressed);
        assert!(state.editor.properties.color.is_none());
        assert_eq!(state.editor.properties.popup, None);
    }

    // -- creation defaults (Task 5) ----------------------------------------

    #[test]
    fn number_creation_copies_current_tool_default() {
        use rollshot_image_document::{NumberSize, NumberStyle};
        let mut state = workspace_with_size(200, 200);
        state.annotation_defaults.values.number.size = NumberSize::Large;
        let _ = update(&mut state, Message::SelectTool(Tool::Number));
        press_move_release(
            &mut state,
            ImagePoint::new(10.0, 10.0),
            ImagePoint::new(10.0, 10.0),
        );
        match &state.document.image.annotations()[0] {
            Annotation::NumberCallout {
                style: NumberStyle { size, .. },
                ..
            } => {
                assert_eq!(*size, NumberSize::Large);
            }
            other => panic!("expected NumberCallout, got {other:?}"),
        }
    }

    #[test]
    fn text_draft_captures_default_style_at_creation_time() {
        use rollshot_image_document::TextSize;
        let mut state = workspace_with_size(200, 100);
        state.annotation_defaults.values.text.font_size = TextSize::Px32;
        let _ = update(&mut state, Message::SelectTool(Tool::Text));
        let _ = update(
            &mut state,
            Message::CanvasPressed(ImagePoint::new(10.0, 10.0)),
        );
        let draft = state.editor.text_draft.as_ref().expect("draft open");
        assert_eq!(draft.style.font_size, TextSize::Px32);
    }

    #[test]
    fn selected_color_preview_changes_canvas_shapes_not_flattened_document() {
        use rollshot_image_document::{annotation_shapes as shapes_fn, Rgb8};
        let mut state = workspace_with_size(200, 200);
        let id = state
            .document
            .image
            .add_number_callout(ImagePoint::new(10.0, 10.0), ImagePoint::new(50.0, 50.0));
        state.editor.tool = Tool::Select;
        state.editor.selection = Some(id);
        let before = state.document.image.flatten();

        let _ = update(
            &mut state,
            Message::OpenColorPicker(super::super::properties::ColorProperty::NumberAccent),
        );
        let _ = update(&mut state, Message::PreviewColor(Rgb8::new(0, 255, 0)));

        let preview =
            super::super::properties::preview_annotation(&state).expect("preview should exist");
        let committed = state.document.image.annotation(id).unwrap();
        assert_ne!(
            shapes_fn(&preview),
            shapes_fn(committed),
            "preview shapes should differ from committed shapes"
        );
        assert_eq!(state.document.image.flatten(), before);
    }

    // -- keyboard precedence and failure behavior (Task 7) --------------------

    /// Build a workspace with all local states layered: color tx, drag,
    /// selection, and a creation tool (Number) active.
    fn workspace_with_all_local_states() -> super::super::ResultWorkspace {
        let mut state = workspace_with_size(200, 200);
        // 1. Add an annotation and select it.
        let id = state
            .document
            .image
            .add_number_callout(ImagePoint::new(10.0, 10.0), ImagePoint::new(10.0, 10.0));
        state.editor.tool = Tool::Select;
        state.editor.selection = Some(id);
        // 2. Switch to Number tool (commits text draft if any — none here).
        let _ = update(&mut state, Message::SelectTool(Tool::Number));
        // 3. Open a color transaction on the active tool.
        let _ = update(
            &mut state,
            Message::OpenColorPicker(super::super::properties::ColorProperty::NumberAccent),
        );
        // 4. Start a creation drag.
        state.editor.drag = Some(super::super::canvas::DragState::CreateNumber {
            tip: ImagePoint::new(50.0, 50.0),
            bubble: ImagePoint::new(60.0, 60.0),
        });
        state
    }

    fn press_escape(state: &mut super::super::ResultWorkspace) {
        let _ = update(state, Message::EscapePressed);
    }

    /// Escape resolves: color tx → drag → selection → creation tool → Select → close.
    #[test]
    fn escape_resolves_property_then_draft_then_selection_then_tool_then_close() {
        let mut state = workspace_with_all_local_states();

        // 1st Esc: cancel the color transaction (highest priority).
        assert!(
            state.editor.properties.color.is_some(),
            "precondition: color tx open"
        );
        press_escape(&mut state);
        assert!(state.editor.properties.color.is_none());
        assert!(
            state.editor.drag.is_some(),
            "drag survives color tx cancellation"
        );

        // 2nd Esc: cancel the in-progress drag.
        press_escape(&mut state);
        assert!(state.editor.drag.is_none());
        assert!(
            state.editor.selection.is_some(),
            "selection survives drag cancellation"
        );

        // 3rd Esc: clear the selection.
        press_escape(&mut state);
        assert!(state.editor.selection.is_none());
        assert_eq!(
            state.editor.tool,
            Tool::Number,
            "tool unchanged after selection clear"
        );

        // 4th Esc: creation tool → Select.
        press_escape(&mut state);
        assert_eq!(state.editor.tool, Tool::Select);

        // 5th Esc: Select tool → trigger close (unsaved workspace).
        press_escape(&mut state);
        assert!(
            state.pending_discard.is_some(),
            "final Esc triggers close on unsaved workspace"
        );
    }

    /// When PropertyFocus is active, Backspace and tool-switching keys must
    /// NOT fire. Escape still works (it is always routed).
    #[test]
    fn focused_property_input_owns_delete_and_shortcut_keys() {
        let mut state = workspace_with_selected_text();
        state.editor.properties.focus = Some(super::super::properties::PropertyFocus::HexInput);

        // DeleteSelected should be suppressed when PropertyFocus is active.
        let before = state.document.image.state_id();
        let _ = update(&mut state, Message::DeleteSelected);
        assert_eq!(state.document.image.state_id(), before, "no deletion");
        assert!(state.editor.selection.is_some(), "selection preserved");

        // Tool-switching should be suppressed.
        let _ = update(&mut state, Message::SelectTool(Tool::Number));
        assert_eq!(state.editor.tool, Tool::Select, "tool not changed");

        // Undo should be suppressed.
        let _ = update(&mut state, Message::Undo);
        assert_eq!(state.document.image.state_id(), before, "no undo");

        // Escape must always work regardless of PropertyFocus.
        let _ = update(&mut state, Message::EscapePressed);
        assert!(
            state.editor.selection.is_none() || state.pending_discard.is_some(),
            "Escape not suppressed by PropertyFocus"
        );
    }

    /// When a defaults save fails, the warning appears once and subsequent
    /// failures keep the memory value without re-warning.
    #[test]
    fn failed_default_save_warns_once_and_keeps_memory_value() {
        use rollshot_image_document::NumberSize;
        let mut state = workspace_with_size(200, 200);
        // Point to a path under /sys which is read-only and cannot be created.
        state.annotation_defaults.config_path = Some(std::path::PathBuf::from(
            "/sys/rollshot_test_dir/config.toml",
        ));
        state.annotation_defaults.warning_reported = false;
        state.editor.tool = Tool::Number;

        // First apply: should warn.
        state.annotation_defaults.values.number.size = NumberSize::Large;
        // Trigger defaults persistence via color apply (NumberTool target).
        let _ = update(
            &mut state,
            Message::OpenColorPicker(super::super::properties::ColorProperty::NumberAccent),
        );
        let _ = update(&mut state, Message::PreviewColor(Rgb8::new(255, 0, 0)));
        let _ = update(&mut state, Message::ApplyColor);

        let first_msg = state.message.as_ref().map(|m| m.text().to_owned());
        assert!(first_msg.is_some(), "first save failure produces a warning");

        // Second apply: message text should be the same (deduped).
        let _ = update(
            &mut state,
            Message::OpenColorPicker(super::super::properties::ColorProperty::NumberAccent),
        );
        let _ = update(&mut state, Message::PreviewColor(Rgb8::new(0, 255, 0)));
        let _ = update(&mut state, Message::ApplyColor);

        let second_msg = state.message.as_ref().map(|m| m.text().to_owned());
        assert_eq!(
            first_msg, second_msg,
            "same warning text on repeated failure"
        );

        // The in-memory value should be updated regardless of persistence failure.
        assert_eq!(
            state.annotation_defaults.values.number.accent,
            Rgb8::new(0, 255, 0),
            "memory value updated despite save failure"
        );
    }

    /// Undo/Redo cancels a property preview before touching document history.
    #[test]
    fn undo_cancels_property_preview_before_history() {
        use rollshot_image_document::Rgb8;
        let mut state = workspace_with_selected_number();
        let before = state.document.image.state_id();

        // Open color preview.
        let _ = update(
            &mut state,
            Message::OpenColorPicker(super::super::properties::ColorProperty::NumberAccent),
        );
        let _ = update(&mut state, Message::PreviewColor(Rgb8::new(10, 20, 30)));
        assert!(state.editor.properties.color.is_some());

        // Undo should cancel the preview (no-op since no doc mutation happened).
        let _ = update(&mut state, Message::Undo);
        assert!(
            state.editor.properties.color.is_none(),
            "undo cancels property preview"
        );
        assert_eq!(state.document.image.state_id(), before);
    }

    /// Invalid next number input leaves state unchanged (no error on empty,
    /// error on non-numeric).
    #[test]
    fn invalid_next_number_leaves_state_unchanged() {
        let mut state = workspace_with_size(200, 200);
        state.editor.tool = Tool::Number;
        let before = state.document.image.next_number();

        // Empty input: no error, no change.
        state.editor.properties.next_number_input.clear();
        let _ = update(&mut state, Message::CommitNextNumber);
        assert_eq!(state.document.image.next_number(), before);
        assert!(state.message.is_none(), "empty input is silent");

        // Non-numeric: error, no change.
        state.editor.properties.next_number_input = "abc".into();
        let _ = update(&mut state, Message::CommitNextNumber);
        assert_eq!(state.document.image.next_number(), before);
        assert!(state.message.as_ref().unwrap().is_error());
    }

    /// Copy/Save payloads exclude live property previews.
    #[test]
    fn copy_and_save_exclude_preview() {
        use rollshot_image_document::Rgb8;
        let mut state = workspace_with_selected_number();

        // Open a color preview that differs from the committed annotation.
        let _ = update(
            &mut state,
            Message::OpenColorPicker(super::super::properties::ColorProperty::NumberAccent),
        );
        let _ = update(&mut state, Message::PreviewColor(Rgb8::new(0, 0, 0)));

        // Copy payload must flatten committed annotations, NOT the preview.
        let copy = copy_payload(&state);
        let flattened = state.document.image.flatten();
        assert_eq!(copy, flattened, "copy payload is flattened document");

        // Save payload must also be the committed document.
        let save = save_payload(&state);
        assert_eq!(save, flattened, "save payload is flattened document");
    }

    /// Opaque Redaction remains fully opaque (alpha=255) after mixed edits.
    #[test]
    fn opaque_redaction_remains_black_after_mixed_edits() {
        let mut state = workspace_with_size(200, 200);
        // Add a number callout.
        state
            .document
            .image
            .add_number_callout(ImagePoint::new(10.0, 10.0), ImagePoint::new(50.0, 50.0));
        // Add a redaction.
        let rid = state
            .document
            .image
            .add_redaction(ImageRect {
                x: 0.0,
                y: 0.0,
                width: 20.0,
                height: 20.0,
            })
            .unwrap();
        // Move the redaction.
        let _ = state.document.image.set_redaction_bounds(
            rid,
            ImageRect {
                x: 5.0,
                y: 5.0,
                width: 25.0,
                height: 25.0,
            },
        );
        // Undo the move.
        let _ = state.document.image.undo();
        // Redo the move.
        let _ = state.document.image.redo();

        // The redaction bounds should still be valid.
        match state.document.image.annotation(rid).unwrap() {
            Annotation::OpaqueRedaction { bounds, .. } => {
                assert_eq!(
                    *bounds,
                    ImageRect {
                        x: 5.0,
                        y: 5.0,
                        width: 25.0,
                        height: 25.0
                    }
                );
            }
            _ => panic!("expected OpaqueRedaction"),
        }

        // Flatten and check pixel inside the redaction: must be fully opaque black.
        let flat = state.document.image.flatten();
        let px = flat.get_pixel(10, 10);
        assert_eq!(px.0[3], 255, "redaction pixel must be fully opaque");
    }

    // -- shape style tests (Task 3) ------------------------------------------

    fn workspace_with_shape(
        kind: rollshot_image_document::ShapeKind,
    ) -> super::super::ResultWorkspace {
        let mut state = workspace_with_size(200, 200);
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
            .unwrap();
        state.editor.tool = Tool::Select;
        state.editor.selection = Some(state.document.image.annotations()[0].id());
        state
    }

    #[test]
    fn shape_preview_only_changes_preview_not_document() {
        let mut state = workspace_with_shape(rollshot_image_document::ShapeKind::Rectangle);
        let _id = state.editor.selection.unwrap();
        let before = state.document.image.state_id();
        let _ = update(&mut state, Message::PreviewShapeStrokeWidth(8.0));
        assert_eq!(state.document.image.state_id(), before);
        let preview = preview_annotation(&state).unwrap();
        assert_eq!(preview.stroke_style().unwrap().width, 8.0);
    }

    #[test]
    fn shape_apply_commits_and_one_undo_restores() {
        let mut state = workspace_with_shape(rollshot_image_document::ShapeKind::Rectangle);
        let id = state.editor.selection.unwrap();
        let _ = update(&mut state, Message::PreviewShapeStrokeWidth(8.0));
        let _ = update(&mut state, Message::ApplyShapeStyle);
        assert_eq!(
            state
                .document
                .image
                .annotation(id)
                .unwrap()
                .stroke_style()
                .unwrap()
                .width,
            8.0
        );
        assert!(state.document.image.undo());
        assert_eq!(
            state
                .document
                .image
                .annotation(id)
                .unwrap()
                .stroke_style()
                .unwrap()
                .width,
            4.0
        );
    }

    #[test]
    fn shape_cancel_discards_preview_without_history() {
        let mut state = workspace_with_shape(rollshot_image_document::ShapeKind::Rectangle);
        let before = state.document.image.state_id();
        let _ = update(&mut state, Message::PreviewShapeStrokeWidth(12.0));
        let _ = update(&mut state, Message::CancelShapeStyle);
        assert!(state.editor.properties.shape_style.is_none());
        assert_eq!(state.document.image.state_id(), before);
    }

    #[test]
    fn toggle_shape_fill_seeds_from_remembered_color() {
        let mut state = workspace_with_shape(rollshot_image_document::ShapeKind::Rectangle);
        let _id = state.editor.selection.unwrap();
        let _ = update(&mut state, Message::ToggleShapeFill);
        let tx = state.editor.properties.shape_style.as_ref().unwrap();
        assert!(tx.preview_fill.is_some());
        assert_eq!(tx.preview_fill.unwrap(), Rgb8::new(0xE5, 0x48, 0x4D));
    }

    #[test]
    fn toggle_shape_fill_disable_reenable_retains_preview_color() {
        let mut state = workspace_with_shape(rollshot_image_document::ShapeKind::Rectangle);
        let _ = update(&mut state, Message::ToggleShapeFill);
        // Change the preview fill color
        if let Some(tx) = &mut state.editor.properties.shape_style {
            tx.preview_fill = Some(Rgb8::new(0, 255, 0));
        }
        let _ = update(&mut state, Message::ToggleShapeFill);
        assert!(state
            .editor
            .properties
            .shape_style
            .as_ref()
            .unwrap()
            .preview_fill
            .is_none());
        let _ = update(&mut state, Message::ToggleShapeFill);
        assert_eq!(
            state
                .editor
                .properties
                .shape_style
                .as_ref()
                .unwrap()
                .preview_fill,
            Some(Rgb8::new(0, 255, 0))
        );
    }

    #[test]
    fn selected_shape_style_does_not_change_defaults() {
        let mut state = workspace_with_shape(rollshot_image_document::ShapeKind::Rectangle);
        let defaults = state.annotation_defaults.values.clone();
        let _ = update(&mut state, Message::PreviewShapeStrokeWidth(12.0));
        let _ = update(&mut state, Message::ApplyShapeStyle);
        assert_eq!(state.annotation_defaults.values, defaults);
    }

    #[test]
    fn selected_shape_colors_compose_until_explicit_shape_apply() {
        use super::super::properties::ColorProperty;

        let mut state = workspace_with_shape(rollshot_image_document::ShapeKind::Rectangle);
        let id = state.editor.selection.unwrap();
        let before = state.document.image.state_id();

        let _ = update(
            &mut state,
            Message::OpenColorPicker(ColorProperty::StrokeColor),
        );
        let _ = update(&mut state, Message::PreviewColor(Rgb8::new(1, 2, 3)));
        let _ = update(&mut state, Message::ApplyColor);

        assert_eq!(state.document.image.state_id(), before);
        assert_eq!(
            preview_annotation(&state)
                .unwrap()
                .stroke_style()
                .unwrap()
                .color,
            Rgb8::new(1, 2, 3)
        );

        let _ = update(&mut state, Message::ToggleShapeFill);
        let _ = update(
            &mut state,
            Message::OpenColorPicker(ColorProperty::ShapeFill),
        );
        let _ = update(&mut state, Message::PreviewColor(Rgb8::new(4, 5, 6)));
        let _ = update(&mut state, Message::ApplyColor);
        let _ = update(&mut state, Message::ApplyShapeStyle);

        match state.document.image.annotation(id).unwrap() {
            Annotation::Shape { stroke, fill, .. } => {
                assert_eq!(stroke.color, Rgb8::new(1, 2, 3));
                assert_eq!(*fill, Some(Rgb8::new(4, 5, 6)));
            }
            _ => panic!("expected Shape"),
        }
    }

    #[test]
    fn selected_shape_hex_color_updates_transaction_preview() {
        use super::super::properties::ColorProperty;

        let mut state = workspace_with_shape(rollshot_image_document::ShapeKind::Rectangle);
        let _ = update(
            &mut state,
            Message::OpenColorPicker(ColorProperty::StrokeColor),
        );
        let _ = update(&mut state, Message::ColorHexChanged("#0A141E".into()));

        assert_eq!(
            preview_annotation(&state)
                .unwrap()
                .stroke_style()
                .unwrap()
                .color,
            Rgb8::new(10, 20, 30)
        );
    }

    #[test]
    fn selected_shape_apply_closes_nested_color_picker() {
        use super::super::properties::ColorProperty;

        let mut state = workspace_with_shape(rollshot_image_document::ShapeKind::Rectangle);
        let _ = update(
            &mut state,
            Message::OpenColorPicker(ColorProperty::StrokeColor),
        );
        let _ = update(&mut state, Message::PreviewColor(Rgb8::new(10, 20, 30)));
        let _ = update(&mut state, Message::ApplyShapeStyle);

        assert!(state.editor.properties.color.is_none());
        assert!(state.editor.properties.popup.is_none());
    }

    #[test]
    fn selected_shape_cancel_closes_nested_color_picker() {
        use super::super::properties::ColorProperty;

        let mut state = workspace_with_shape(rollshot_image_document::ShapeKind::Rectangle);
        let before = state.document.image.state_id();
        let _ = update(
            &mut state,
            Message::OpenColorPicker(ColorProperty::ShapeFill),
        );
        let _ = update(&mut state, Message::PreviewColor(Rgb8::new(10, 20, 30)));
        let _ = update(&mut state, Message::CancelShapeStyle);

        assert!(state.editor.properties.shape_style.is_none());
        assert!(state.editor.properties.color.is_none());
        assert!(state.editor.properties.popup.is_none());
        assert_eq!(state.document.image.state_id(), before);
    }

    #[test]
    fn shape_tool_width_apply_updates_only_active_defaults() {
        let mut state = workspace();
        state.editor.tool = Tool::Rectangle;
        let ellipse_before = state.annotation_defaults.values.ellipse;

        let _ = update(&mut state, Message::PreviewShapeStrokeWidth(9.0));
        let _ = update(&mut state, Message::ApplyShapeStyle);

        assert_eq!(state.annotation_defaults.values.rectangle.stroke.width, 9.0);
        assert_eq!(state.annotation_defaults.values.ellipse, ellipse_before);
    }

    #[test]
    fn shape_tool_fill_color_apply_updates_remembered_default() {
        use super::super::properties::ColorProperty;

        let mut state = workspace();
        state.editor.tool = Tool::Ellipse;
        let rectangle_before = state.annotation_defaults.values.rectangle;

        let _ = update(&mut state, Message::ToggleShapeFill);
        let _ = update(
            &mut state,
            Message::OpenColorPicker(ColorProperty::ShapeFill),
        );
        let _ = update(&mut state, Message::PreviewColor(Rgb8::new(7, 8, 9)));
        let _ = update(&mut state, Message::ApplyColor);

        assert_eq!(
            state.annotation_defaults.values.ellipse.fill_color,
            Rgb8::new(7, 8, 9)
        );
        assert_eq!(state.annotation_defaults.values.rectangle, rectangle_before);
    }

    #[test]
    fn shape_tool_switch_discards_preview() {
        let mut state = workspace_with_shape(rollshot_image_document::ShapeKind::Rectangle);
        let _ = update(&mut state, Message::PreviewShapeStrokeWidth(12.0));
        let _ = update(&mut state, Message::SelectTool(Tool::Arrow));
        assert!(state.editor.properties.shape_style.is_none());
    }

    #[test]
    fn shape_escape_discards_preview() {
        let mut state = workspace_with_shape(rollshot_image_document::ShapeKind::Rectangle);
        let _ = update(&mut state, Message::PreviewShapeStrokeWidth(12.0));
        let _ = update(&mut state, Message::EscapePressed);
        assert!(state.editor.properties.shape_style.is_none());
    }

    #[test]
    fn shape_undo_discards_preview() {
        let mut state = workspace_with_shape(rollshot_image_document::ShapeKind::Rectangle);
        let _ = update(&mut state, Message::PreviewShapeStrokeWidth(12.0));
        let _ = update(&mut state, Message::Undo);
        assert!(state.editor.properties.shape_style.is_none());
    }

    #[test]
    fn apply_shape_style_with_stale_id_leaves_state_unchanged() {
        use super::super::properties::ShapeStyleTransaction;

        let mut state = workspace_with_shape(rollshot_image_document::ShapeKind::Rectangle);
        let defaults_before = state.annotation_defaults.values.clone();
        let history_before = state.document.image.state_id();
        let doc_before = state.document.image.flatten();

        // Inject a transaction referencing a nonexistent annotation.
        let stale_id = AnnotationId(9999);
        state.editor.properties.shape_style = Some(ShapeStyleTransaction {
            id: stale_id,
            kind: rollshot_image_document::ShapeKind::Rectangle,
            original_stroke: StrokeStyle {
                color: Rgb8::new(0, 0, 0),
                width: 1.0,
                opacity: 1.0,
            },
            original_fill: None,
            preview_stroke: StrokeStyle {
                color: Rgb8::new(255, 0, 0),
                width: 99.0,
                opacity: 1.0,
            },
            preview_fill: Some(Rgb8::new(0, 255, 0)),
            remembered_fill_color: Rgb8::new(0, 0, 0),
        });

        let _ = update(&mut state, Message::ApplyShapeStyle);

        // Transaction should remain (set_shape_style errored, take() was called
        // but the error is recorded as inline message).
        assert!(state.editor.properties.shape_style.is_none());
        assert!(matches!(
            state.message,
            Some(InlineMessage::Error(ref e)) if e.contains("annotation does not exist")
        ));
        // Document, history, and defaults must be unchanged.
        assert_eq!(state.document.image.state_id(), history_before);
        assert_eq!(state.annotation_defaults.values, defaults_before);
        let doc_after = state.document.image.flatten();
        assert_eq!(doc_before.dimensions(), doc_after.dimensions());
        // Compare a sample pixel to confirm no mutation.
        let px_before = doc_before.get_pixel(5, 5);
        let px_after = doc_after.get_pixel(5, 5);
        assert_eq!(px_before, px_after);
    }

    // -- shapes selector shortcuts (Task 5) -----------------------------------

    #[test]
    fn select_shape_activates_tool_and_persists_last_shape() {
        let mut state = workspace();
        let _ = update(
            &mut state,
            Message::SelectShape(rollshot_image_document::ShapeKind::Ellipse),
        );
        assert_eq!(state.editor.tool, Tool::Ellipse);
        assert_eq!(
            state.annotation_defaults.values.last_shape,
            rollshot_image_document::ShapeKind::Ellipse
        );
    }

    #[test]
    fn select_shape_closes_shapes_menu() {
        let mut state = workspace();
        state.editor.shapes_menu_open = true;
        let _ = update(
            &mut state,
            Message::SelectShape(rollshot_image_document::ShapeKind::Rectangle),
        );
        assert!(!state.editor.shapes_menu_open);
    }

    #[test]
    fn selector_shape_switch_discards_selected_shape_transaction() {
        let mut state = workspace_with_shape(rollshot_image_document::ShapeKind::Rectangle);
        let _ = update(&mut state, Message::PreviewShapeStrokeWidth(12.0));

        let _ = update(
            &mut state,
            Message::SelectShape(rollshot_image_document::ShapeKind::Ellipse),
        );

        assert_eq!(state.editor.tool, Tool::Ellipse);
        assert!(state.editor.properties.shape_style.is_none());
        assert!(state.editor.properties.color.is_none());
    }

    #[test]
    fn remembered_shape_switch_discards_other_shape_default_transaction() {
        let mut state = workspace();
        state.editor.tool = Tool::Rectangle;
        let _ = update(&mut state, Message::PreviewShapeStrokeWidth(12.0));
        state.annotation_defaults.values.last_shape = rollshot_image_document::ShapeKind::Ellipse;

        let _ = update(&mut state, Message::SelectRememberedShape);

        assert_eq!(state.editor.tool, Tool::Ellipse);
        assert!(state.editor.properties.shape_style.is_none());
        assert_eq!(state.annotation_defaults.values.rectangle.stroke.width, 4.0);
        assert_eq!(state.annotation_defaults.values.ellipse.stroke.width, 4.0);
    }

    #[test]
    fn select_shape_does_not_alter_other_kind_defaults() {
        let mut state = workspace();
        let rect_before = state.annotation_defaults.values.rectangle;
        let _ = update(
            &mut state,
            Message::SelectShape(rollshot_image_document::ShapeKind::Ellipse),
        );
        assert_eq!(state.annotation_defaults.values.rectangle, rect_before);
    }

    #[test]
    fn select_remembered_shape_activates_without_cycling() {
        let mut state = workspace();
        // Default remembered = Rectangle
        let _ = update(&mut state, Message::SelectRememberedShape);
        assert_eq!(state.editor.tool, Tool::Rectangle);
        // Change remembered to Ellipse
        state.annotation_defaults.values.last_shape = rollshot_image_document::ShapeKind::Ellipse;
        let _ = update(&mut state, Message::SelectRememberedShape);
        assert_eq!(state.editor.tool, Tool::Ellipse);
    }

    #[test]
    fn toggle_shapes_menu_toggles_open_close() {
        let mut state = workspace();
        assert!(!state.editor.shapes_menu_open);
        let _ = update(&mut state, Message::ToggleShapesMenu);
        assert!(state.editor.shapes_menu_open);
        let _ = update(&mut state, Message::ToggleShapesMenu);
        assert!(!state.editor.shapes_menu_open);
    }

    #[test]
    fn toggle_shapes_menu_closes_other_menus() {
        let mut state = workspace();
        state.editor.more_menu_open = true;
        state.editor.copy_menu_open = true;
        let _ = update(&mut state, Message::ToggleShapesMenu);
        assert!(!state.editor.more_menu_open);
        assert!(!state.editor.copy_menu_open);
    }

    #[test]
    fn toggle_shapes_menu_changes_no_tool_or_dirty_state() {
        let mut state = workspace();
        let tool_before = state.editor.tool;
        let dirty_before = state.annotations_dirty();
        let _ = update(&mut state, Message::ToggleShapesMenu);
        assert_eq!(state.editor.tool, tool_before);
        assert_eq!(state.annotations_dirty(), dirty_before);
        let _ = update(&mut state, Message::ToggleShapesMenu);
        assert_eq!(state.editor.tool, tool_before);
        assert_eq!(state.annotations_dirty(), dirty_before);
    }

    #[test]
    fn s_shortcut_maps_to_select_remembered_shape() {
        use keyboard::Key;
        let none = keyboard::Modifiers::default();
        assert_eq!(
            map_key_press(&Key::Character("s".into()), none, false),
            Some(Message::SelectRememberedShape)
        );
    }

    #[test]
    fn command_s_does_not_activate_tool() {
        use keyboard::Key;
        assert_eq!(
            map_key_press(&Key::Character("s".into()), zmod(), false),
            None
        );
    }

    #[test]
    fn captured_s_suppresses_shortcut() {
        use keyboard::Key;
        let none = keyboard::Modifiers::default();
        assert_eq!(map_key_press(&Key::Character("s".into()), none, true), None);
    }

    #[test]
    fn select_tool_closes_shapes_menu() {
        let mut state = workspace();
        state.editor.shapes_menu_open = true;
        let _ = update(&mut state, Message::SelectTool(Tool::Text));
        assert!(!state.editor.shapes_menu_open);
    }

    #[test]
    fn more_menu_open_closes_shapes_menu() {
        let mut state = workspace();
        state.editor.shapes_menu_open = true;
        let _ = update(&mut state, Message::ToggleMoreMenu);
        assert!(!state.editor.shapes_menu_open);
    }

    #[test]
    fn copy_menu_open_closes_shapes_menu() {
        let mut state = workspace();
        state.editor.shapes_menu_open = true;
        let _ = update(&mut state, Message::ToggleCopyMenu);
        assert!(!state.editor.shapes_menu_open);
    }

    #[test]
    fn escape_closes_shapes_menu() {
        let mut state = workspace();
        state.editor.shapes_menu_open = true;
        let _ = update(&mut state, Message::EscapePressed);
        assert!(!state.editor.shapes_menu_open);
    }

    #[test]
    fn selection_change_closes_shapes_menu() {
        let mut state = workspace();
        let id = state.document.image.add_number_callout(
            rollshot_image_document::ImagePoint::new(10.0, 10.0),
            rollshot_image_document::ImagePoint::new(10.0, 10.0),
        );
        state.editor.shapes_menu_open = true;
        set_selection(&mut state, Some(id));
        assert!(!state.editor.shapes_menu_open);
    }

    #[test]
    fn color_picker_open_closes_shapes_menu() {
        let mut state = workspace();
        state.editor.tool = Tool::Number;
        state.editor.shapes_menu_open = true;
        let _ = update(
            &mut state,
            Message::OpenColorPicker(super::super::properties::ColorProperty::NumberAccent),
        );
        assert!(!state.editor.shapes_menu_open);
    }

    // -- shape output and lifecycle (Task 6) ----------------------------------

    #[test]
    fn shape_output_committed_rectangle_and_ellipse_appear_in_copy_and_save() {
        let mut state = workspace_with_size(200, 200);
        state
            .document
            .image
            .add_shape(
                rollshot_image_document::ShapeKind::Rectangle,
                ImageRect {
                    x: 10.0,
                    y: 10.0,
                    width: 50.0,
                    height: 50.0,
                },
            )
            .unwrap();
        state
            .document
            .image
            .add_shape(
                rollshot_image_document::ShapeKind::Ellipse,
                ImageRect {
                    x: 100.0,
                    y: 100.0,
                    width: 50.0,
                    height: 50.0,
                },
            )
            .unwrap();

        let source = state.document.image.source();
        let copy = copy_payload(&state);
        let save = save_payload(&state);
        let flattened = state.document.image.flatten();

        assert_eq!(copy, flattened, "copy payload equals flatten");
        assert_eq!(save, flattened, "save payload equals flatten");

        // Rectangle stroke at left edge (10, 35).
        assert_ne!(
            copy.get_pixel(10, 35),
            source.get_pixel(10, 35),
            "committed Rectangle stroke must appear in output"
        );
        // Rectangle interior without fill must remain source.
        assert_eq!(
            copy.get_pixel(35, 35),
            source.get_pixel(35, 35),
            "Rectangle interior without fill must be source"
        );
        // Ellipse stroke at top center (125, 100).
        assert_ne!(
            copy.get_pixel(125, 100),
            source.get_pixel(125, 100),
            "committed Ellipse stroke must appear in output"
        );
        // Ellipse center without fill must remain source.
        assert_eq!(
            copy.get_pixel(125, 125),
            source.get_pixel(125, 125),
            "Ellipse center without fill must be source"
        );
    }

    #[test]
    fn shape_output_excludes_draft_outline_and_handles() {
        let mut state = workspace_with_size(200, 200);
        // Commit a Rectangle to give us something to select.
        let rect_id = state
            .document
            .image
            .add_shape(
                rollshot_image_document::ShapeKind::Rectangle,
                ImageRect {
                    x: 10.0,
                    y: 10.0,
                    width: 50.0,
                    height: 50.0,
                },
            )
            .unwrap();
        state.editor.tool = Tool::Select;
        state.editor.selection = Some(rect_id);

        // Start an in-progress shape creation (draft, not committed).
        state.editor.tool = Tool::Rectangle;
        let _ = handle_canvas_pressed(&mut state, ImagePoint::new(150.0, 150.0), Instant::now());
        let _ = handle_canvas_moved(&mut state, ImagePoint::new(180.0, 180.0));
        assert!(matches!(
            state.editor.drag,
            Some(DragState::CreateShape { .. })
        ));

        let source = state.document.image.source();
        let output = copy_payload(&state);
        let flattened = state.document.image.flatten();
        assert_eq!(output, flattened, "copy equals flatten");

        // Draft center (165, 165) must NOT appear in output.
        assert_eq!(
            output.get_pixel(165, 165),
            source.get_pixel(165, 165),
            "uncommitted shape draft must stay out of output"
        );
        // Selection handles are app-only canvas visuals and must NOT appear
        // in flattened output. Pick a pixel near the committed Rectangle
        // (10,10)-(60,60) that is inside the handle circle radius but outside
        // the committed stroke's outer boundary (8,8)-(62,62).
        assert_eq!(
            output.get_pixel(5, 5),
            source.get_pixel(5, 5),
            "selection handle area must stay out of output"
        );
    }

    #[test]
    fn failed_shape_operations_do_not_change_state() {
        let mut state = workspace_with_size(200, 200);
        let before_id = state.document.image.state_id();
        let before_selection = state.editor.selection;
        let before_defaults = state.annotation_defaults.values.clone();

        // 1. Apply shape style to a stale (nonexistent) annotation.
        state.editor.properties.shape_style =
            Some(super::super::properties::ShapeStyleTransaction {
                id: AnnotationId(9999),
                kind: rollshot_image_document::ShapeKind::Rectangle,
                original_stroke: StrokeStyle::default(),
                original_fill: None,
                preview_stroke: StrokeStyle {
                    color: Rgb8::new(255, 0, 0),
                    width: 99.0,
                    opacity: 1.0,
                },
                preview_fill: None,
                remembered_fill_color: Rgb8::new(0, 0, 0),
            });
        let _ = update(&mut state, Message::ApplyShapeStyle);
        assert_eq!(
            state.document.image.state_id(),
            before_id,
            "stale ID: state_id unchanged"
        );
        assert!(!state.annotations_dirty(), "stale ID: not dirty");
        assert_eq!(
            state.editor.selection, before_selection,
            "stale ID: selection unchanged"
        );
        assert_eq!(
            state.annotation_defaults.values, before_defaults,
            "stale ID: defaults unchanged"
        );
        assert!(state.editor.properties.shape_style.is_none());

        // 2. Cancel a shape style preview without applying.
        let mut state2 = workspace_with_shape(rollshot_image_document::ShapeKind::Rectangle);
        state2.editor.saved_state_id = state2.document.image.state_id();
        let before2 = state2.document.image.state_id();
        let _ = update(&mut state2, Message::PreviewShapeStrokeWidth(12.0));
        let _ = update(&mut state2, Message::CancelShapeStyle);
        assert_eq!(
            state2.document.image.state_id(),
            before2,
            "cancel: state_id unchanged"
        );
        assert!(!state2.annotations_dirty(), "cancel: not dirty");
    }

    #[test]
    fn successful_shape_edit_updates_state_id_and_dirty() {
        let mut state = workspace_with_shape(rollshot_image_document::ShapeKind::Rectangle);
        // Mark the workspace as clean (saved at current state).
        state.editor.saved_state_id = state.document.image.state_id();
        let before = state.document.image.state_id();
        assert!(!state.annotations_dirty());

        let _ = update(&mut state, Message::PreviewShapeStrokeWidth(8.0));
        let _ = update(&mut state, Message::ApplyShapeStyle);
        assert_ne!(
            state.document.image.state_id(),
            before,
            "apply: state_id updated"
        );
        assert!(state.annotations_dirty(), "apply: dirty");
    }

    // -- freehand creation gesture (Task 7) ---------------------------------

    #[test]
    fn freehand_gesture_filters_samples_and_commits_simplified_stroke() {
        let mut state = workspace_with_size(200, 200);
        state.editor.tool = Tool::Pen;
        let t0 = Instant::now();
        let _ = handle_canvas_pressed(&mut state, ImagePoint::new(10.0, 10.0), t0);
        for i in 1..=30 {
            let _ = handle_canvas_moved(&mut state, ImagePoint::new(10.0 + i as f32, 10.0));
        }
        let _ = handle_canvas_released(&mut state, ImagePoint::new(40.0, 10.0));
        let annotations = state.document.image.annotations();
        assert_eq!(annotations.len(), 1);
        match &annotations[0] {
            Annotation::Freehand { kind, points, .. } => {
                assert_eq!(*kind, rollshot_image_document::FreehandKind::Pen);
                assert_eq!(
                    points.len(),
                    2,
                    "collinear stroke must simplify to endpoints"
                );
                assert_eq!(points[0], ImagePoint::new(10.0, 10.0));
                assert_eq!(points[1], ImagePoint::new(40.0, 10.0));
            }
            other => panic!("expected freehand, got {other:?}"),
        }
        assert!(state.document.image.can_undo());
        state.document.image.undo();
        assert!(!state.document.image.can_undo());
    }

    #[test]
    fn freehand_click_and_subthreshold_gestures_cancel() {
        let mut state = workspace_with_size(200, 200);
        state.editor.tool = Tool::Highlighter;
        let t0 = Instant::now();
        // Plain click.
        let _ = handle_canvas_pressed(&mut state, ImagePoint::new(10.0, 10.0), t0);
        let _ = handle_canvas_released(&mut state, ImagePoint::new(10.0, 10.0));
        // 2-px wiggle (below the 4-screen-px threshold at scale 1).
        let _ = handle_canvas_pressed(&mut state, ImagePoint::new(10.0, 10.0), t0);
        let _ = handle_canvas_moved(&mut state, ImagePoint::new(12.0, 10.0));
        let _ = handle_canvas_released(&mut state, ImagePoint::new(12.0, 10.0));
        assert!(state.document.image.annotations().is_empty());
        assert!(!state.document.image.can_undo());
    }

    #[test]
    fn highlighter_stroke_uses_highlighter_defaults() {
        let mut state = workspace_with_size(200, 200);
        state.editor.tool = Tool::Highlighter;
        let t0 = Instant::now();
        let _ = handle_canvas_pressed(&mut state, ImagePoint::new(10.0, 10.0), t0);
        let _ = handle_canvas_moved(&mut state, ImagePoint::new(40.0, 20.0));
        let _ = handle_canvas_released(&mut state, ImagePoint::new(40.0, 20.0));
        match &state.document.image.annotations()[0] {
            Annotation::Freehand { style, .. } => {
                assert_eq!(*style, StrokeStyle::highlighter_default());
            }
            other => panic!("expected freehand, got {other:?}"),
        }
    }

    #[test]
    fn esc_clears_active_freehand_draft() {
        let mut state = workspace_with_size(200, 200);
        state.editor.tool = Tool::Pen;
        let t0 = Instant::now();
        let _ = handle_canvas_pressed(&mut state, ImagePoint::new(10.0, 10.0), t0);
        let _ = handle_canvas_moved(&mut state, ImagePoint::new(80.0, 40.0));
        assert!(matches!(
            state.editor.drag,
            Some(DragState::CreateFreehand { .. })
        ));
        let _ = update(&mut state, Message::EscapePressed);
        assert!(state.editor.drag.is_none());
        assert!(state.document.image.annotations().is_empty());
        assert!(!state.document.image.can_undo());
        assert_eq!(state.editor.tool, Tool::Pen);
    }

    #[test]
    fn out_of_bounds_pointer_moves_and_release_are_clamped() {
        let mut state = workspace_with_size(100, 100);
        state.editor.tool = Tool::Pen;
        let t0 = Instant::now();
        let _ = handle_canvas_pressed(&mut state, ImagePoint::new(50.0, 50.0), t0);
        // Move past right edge.
        let _ = handle_canvas_moved(&mut state, ImagePoint::new(150.0, 50.0));
        // Move past bottom edge.
        let _ = handle_canvas_moved(&mut state, ImagePoint::new(50.0, 150.0));
        let _ = handle_canvas_released(&mut state, ImagePoint::new(150.0, 150.0));
        assert!(
            !state.document.image.annotations().is_empty(),
            "out-of-bounds gesture must still produce an annotation"
        );
        match &state.document.image.annotations()[0] {
            Annotation::Freehand { points, .. } => {
                for p in points {
                    assert!(p.x <= 100.0 && p.y <= 100.0, "point {p:?} out of bounds");
                }
            }
            other => panic!("expected freehand, got {other:?}"),
        }
    }

    #[test]
    fn select_body_movement_commits_one_history_entry_and_undo_restores_all_points() {
        let mut state = workspace_with_size(200, 200);
        let original_points = vec![
            ImagePoint::new(10.0, 10.0),
            ImagePoint::new(20.0, 30.0),
            ImagePoint::new(40.0, 20.0),
        ];
        let id = state
            .document
            .image
            .add_freehand_with_style(
                rollshot_image_document::FreehandKind::Pen,
                original_points.clone(),
                StrokeStyle::default(),
            )
            .unwrap();
        state.editor.tool = Tool::Select;
        state.editor.selection = Some(id);
        let before = state.document.image.state_id();

        // Grab the body and drag.
        let _ = handle_canvas_pressed(&mut state, ImagePoint::new(10.0, 10.0), Instant::now());
        let _ = handle_canvas_moved(&mut state, ImagePoint::new(20.0, 20.0));
        let _ = handle_canvas_released(&mut state, ImagePoint::new(20.0, 20.0));

        assert_ne!(state.document.image.state_id(), before);
        // Exactly one history entry for the movement (plus one for creation).
        assert!(state.document.image.can_undo());
        state.document.image.undo(); // undo movement
                                     // Points restored.
        match state.document.image.annotation(id).unwrap() {
            Annotation::Freehand { points, .. } => {
                assert_eq!(*points, original_points);
            }
            other => panic!("expected freehand, got {other:?}"),
        }
        assert!(state.document.image.can_undo());
        state.document.image.undo(); // undo creation
        assert!(!state.document.image.can_undo());
    }

    #[test]
    fn release_within_filter_does_not_append_redundant_endpoint() {
        let mut state = workspace_with_size(200, 200);
        state.editor.tool = Tool::Pen;
        let t0 = Instant::now();
        let _ = handle_canvas_pressed(&mut state, ImagePoint::new(10.0, 10.0), t0);
        // Move to 30,10 — 20px gesture, well above the 4px threshold.
        let _ = handle_canvas_moved(&mut state, ImagePoint::new(30.0, 10.0));
        // Release at 31,10 — within 2 screen px filter of last accepted point
        // (distance = 1.0 < 2.0), so it should NOT be appended.
        let _ = handle_canvas_released(&mut state, ImagePoint::new(31.0, 10.0));
        assert!(
            !state.document.image.annotations().is_empty(),
            "20px gesture must produce an annotation"
        );
        match &state.document.image.annotations()[0] {
            Annotation::Freehand { points, .. } => {
                // RDP simplifies the collinear stroke to endpoints.
                assert_eq!(
                    points.len(),
                    2,
                    "collinear stroke should simplify to 2 points"
                );
                assert_eq!(points[0], ImagePoint::new(10.0, 10.0));
                // Second point must be (30, 10), NOT the filtered (31, 10).
                assert_eq!(
                    points[1],
                    ImagePoint::new(30.0, 10.0),
                    "release point within 2px filter should not be appended"
                );
            }
            other => panic!("expected freehand, got {other:?}"),
        }
    }

    #[test]
    fn successful_creation_marks_dirty_and_refreshes_navigator_and_keeps_tool() {
        let mut state = workspace_with_size(200, 200);
        state.editor.saved_state_id = state.document.image.state_id();
        state.editor.tool = Tool::Pen;
        let t0 = Instant::now();
        let _ = handle_canvas_pressed(&mut state, ImagePoint::new(10.0, 10.0), t0);
        let _ = handle_canvas_moved(&mut state, ImagePoint::new(60.0, 10.0));
        let _ = handle_canvas_released(&mut state, ImagePoint::new(60.0, 10.0));

        assert!(state.annotations_dirty());
        let items = state.document.image.navigator_items();
        assert_eq!(items.len(), 1);
        assert_eq!(
            state.editor.tool,
            Tool::Pen,
            "tool stays active after creation"
        );
        assert_eq!(state.editor.selection, None, "new stroke is not selected");
    }

    // -- opacity transaction tests (Task 8) ----------------------------------

    #[test]
    fn apply_opacity_to_highlighter_defaults_persists() {
        let mut state = workspace();
        state.editor.tool = Tool::Highlighter;
        let _ = update(&mut state, Message::PreviewStrokeOpacity(0.7));
        let _ = update(&mut state, Message::ApplyStrokeOpacity);
        assert_eq!(state.annotation_defaults.values.highlighter.opacity, 0.7);
    }

    #[test]
    fn apply_opacity_to_selected_highlighter_is_one_undo_step() {
        let mut state = workspace();
        let id = state
            .document
            .image
            .add_freehand_with_style(
                rollshot_image_document::FreehandKind::Highlighter,
                vec![ImagePoint::new(0.0, 0.0), ImagePoint::new(10.0, 10.0)],
                StrokeStyle::highlighter_default(),
            )
            .unwrap();
        state.editor.tool = Tool::Select;
        state.editor.selection = Some(id);
        let _ = update(&mut state, Message::PreviewStrokeOpacity(0.9));
        let _ = update(&mut state, Message::ApplyStrokeOpacity);
        assert_eq!(
            state
                .document
                .image
                .annotation(id)
                .unwrap()
                .stroke_style()
                .unwrap()
                .opacity,
            0.9
        );
        state.document.image.undo();
        assert_eq!(
            state
                .document
                .image
                .annotation(id)
                .unwrap()
                .stroke_style()
                .unwrap()
                .opacity,
            0.4
        );
    }

    #[test]
    fn opacity_never_targets_pen() {
        let mut state = workspace();
        state.editor.tool = Tool::Pen;
        let _ = update(&mut state, Message::PreviewStrokeOpacity(0.5));
        assert!(state.editor.properties.opacity.is_none());
        let _ = update(&mut state, Message::ApplyStrokeOpacity);
        assert_eq!(state.annotation_defaults.values.pen.opacity, 1.0);
    }

    #[test]
    fn opacity_cancel_discards_preview_without_changing_defaults_or_document() {
        let mut state = workspace();
        state.editor.tool = Tool::Highlighter;
        let before_defaults = state.annotation_defaults.values.clone();
        let _ = update(&mut state, Message::PreviewStrokeOpacity(0.6));
        assert!(state.editor.properties.opacity.is_some());
        let _ = update(&mut state, Message::CancelStrokeOpacity);
        assert!(state.editor.properties.opacity.is_none());
        assert_eq!(state.annotation_defaults.values, before_defaults);
    }

    #[test]
    fn esc_clears_opacity_transaction_preserving_tool_and_selection() {
        let mut state = workspace();
        state.editor.tool = Tool::Highlighter;
        let _ = update(&mut state, Message::PreviewStrokeOpacity(0.6));
        assert!(state.editor.properties.opacity.is_some());
        let _ = update(&mut state, Message::EscapePressed);
        assert!(state.editor.properties.opacity.is_none());
        assert_eq!(
            state.editor.tool,
            Tool::Highlighter,
            "Esc cancelling opacity must not switch the tool"
        );
    }

    #[test]
    fn tool_switch_clears_opacity_transaction() {
        let mut state = workspace();
        state.editor.tool = Tool::Highlighter;
        let _ = update(&mut state, Message::PreviewStrokeOpacity(0.6));
        assert!(state.editor.properties.opacity.is_some());
        let _ = update(&mut state, Message::SelectTool(Tool::Pen));
        assert!(state.editor.properties.opacity.is_none());
    }

    #[test]
    fn starting_opacity_preview_clears_width_and_color() {
        use super::super::properties::{
            ColorProperty, ColorTransaction, PropertyTarget, StrokeWidthTransaction,
        };

        let mut state = workspace();
        state.editor.tool = Tool::Highlighter;
        state.editor.properties.width = Some(StrokeWidthTransaction {
            target: PropertyTarget::FreehandTool(
                rollshot_image_document::FreehandKind::Highlighter,
            ),
            original: 12.0,
            preview: 16.0,
        });
        state.editor.properties.color = Some(ColorTransaction {
            target: PropertyTarget::FreehandTool(
                rollshot_image_document::FreehandKind::Highlighter,
            ),
            property: ColorProperty::StrokeColor,
            original: Rgb8::new(0, 0, 0),
            preview: Rgb8::new(255, 0, 0),
            hex: "#FF0000".into(),
        });
        let _ = update(&mut state, Message::PreviewStrokeOpacity(0.5));
        assert!(state.editor.properties.width.is_none());
        assert!(state.editor.properties.color.is_none());
        assert!(state.editor.properties.opacity.is_some());
    }

    #[test]
    fn starting_width_preview_clears_opacity() {
        use super::super::properties::{OpacityTransaction, PropertyTarget};

        let mut state = workspace();
        state.editor.tool = Tool::Highlighter;
        state.editor.properties.opacity = Some(OpacityTransaction {
            target: PropertyTarget::FreehandTool(
                rollshot_image_document::FreehandKind::Highlighter,
            ),
            original: 0.4,
            preview: 0.8,
        });
        let _ = update(&mut state, Message::PreviewStrokeWidth(16.0));
        assert!(state.editor.properties.opacity.is_none());
        assert!(state.editor.properties.width.is_some());
    }

    #[test]
    fn nan_opacity_is_rejected() {
        let mut state = workspace();
        state.editor.tool = Tool::Highlighter;
        let _ = update(&mut state, Message::PreviewStrokeOpacity(f32::NAN));
        assert!(state.editor.properties.opacity.is_none());
    }

    #[test]
    fn infinite_opacity_is_rejected() {
        let mut state = workspace();
        state.editor.tool = Tool::Highlighter;
        let _ = update(&mut state, Message::PreviewStrokeOpacity(f32::INFINITY));
        assert!(state.editor.properties.opacity.is_none());
    }

    #[test]
    fn out_of_range_opacity_clamps_to_valid_range() {
        let mut state = workspace();
        state.editor.tool = Tool::Highlighter;
        let _ = update(&mut state, Message::PreviewStrokeOpacity(0.01));
        assert_eq!(
            state.editor.properties.opacity.as_ref().unwrap().preview,
            0.1
        );
        let _ = update(&mut state, Message::PreviewStrokeOpacity(5.0));
        assert_eq!(
            state.editor.properties.opacity.as_ref().unwrap().preview,
            1.0
        );
    }

    // -- pixelate creation gesture (Task 5) ----------------------------------

    #[test]
    fn pixelate_release_commits_once_and_keeps_tool_active() {
        let mut state = workspace_with_size(100, 100);
        state.editor.tool = Tool::Pixelate;
        press_move_release(
            &mut state,
            ImagePoint::new(10.0, 12.0),
            ImagePoint::new(40.0, 32.0),
        );
        assert_eq!(state.editor.tool, Tool::Pixelate);
        assert_eq!(state.editor.selection, None);
        assert_eq!(state.document.image.annotations().len(), 1);
        assert!(
            matches!(state.document.image.annotations()[0], Annotation::Pixelate { bounds, block_size: 16, .. } if bounds == ImageRect { x: 10.0, y: 12.0, width: 30.0, height: 20.0 })
        );
        assert!(state.document.image.undo());
        assert!(state.document.image.annotations().is_empty());
    }

    #[test]
    fn pixelate_reverse_drag_normalizes_bounds() {
        let mut state = workspace_with_size(100, 100);
        state.editor.tool = Tool::Pixelate;
        press_move_release(
            &mut state,
            ImagePoint::new(40.0, 32.0),
            ImagePoint::new(10.0, 12.0),
        );
        assert_eq!(state.document.image.annotations().len(), 1);
        assert!(
            matches!(state.document.image.annotations()[0], Annotation::Pixelate { bounds, .. } if bounds == ImageRect { x: 10.0, y: 12.0, width: 30.0, height: 20.0 })
        );
    }

    #[test]
    fn pixelate_shift_creates_square() {
        let mut state = workspace_with_size(100, 100);
        state.editor.tool = Tool::Pixelate;
        let _ = update(
            &mut state,
            Message::ModifiersChanged(iced::keyboard::Modifiers::SHIFT),
        );
        press_move_release(
            &mut state,
            ImagePoint::new(10.0, 10.0),
            ImagePoint::new(40.0, 25.0),
        );
        assert_eq!(state.document.image.annotations().len(), 1);
        match &state.document.image.annotations()[0] {
            Annotation::Pixelate { bounds, .. } => {
                assert_eq!(bounds.width, bounds.height, "shift must produce square");
                assert_eq!(bounds.width, 15.0, "min delta = 15");
            }
            other => panic!("expected Pixelate, got {other:?}"),
        }
    }

    #[test]
    fn pixelate_click_sub_threshold_creates_nothing() {
        let mut state = workspace_with_size(100, 100);
        state.editor.tool = Tool::Pixelate;
        press_move_release(
            &mut state,
            ImagePoint::new(10.0, 10.0),
            ImagePoint::new(10.0, 10.0),
        );
        assert!(state.document.image.annotations().is_empty());
        assert!(!state.document.image.can_undo());
    }

    #[test]
    fn pixelate_escape_cancels_creation_without_history() {
        let mut state = workspace_with_size(100, 100);
        state.editor.tool = Tool::Pixelate;
        let _ = handle_canvas_pressed(&mut state, ImagePoint::new(10.0, 10.0), Instant::now());
        let _ = handle_canvas_moved(&mut state, ImagePoint::new(40.0, 40.0));
        assert!(matches!(
            state.editor.drag,
            Some(DragState::CreatePixelate { .. })
        ));
        let _ = update(&mut state, Message::EscapePressed);
        assert!(state.editor.drag.is_none());
        assert!(state.document.image.annotations().is_empty());
        assert!(!state.document.image.can_undo());
        assert_eq!(state.editor.tool, Tool::Pixelate);
    }

    #[test]
    fn pixelate_creation_over_existing_annotation_does_not_edit_or_select_it() {
        let mut state = workspace_with_size(100, 100);
        let existing_id = state
            .document
            .image
            .add_number_callout(ImagePoint::new(10.0, 10.0), ImagePoint::new(10.0, 10.0));
        state.editor.tool = Tool::Pixelate;
        press_move_release(
            &mut state,
            ImagePoint::new(5.0, 5.0),
            ImagePoint::new(30.0, 30.0),
        );
        assert_eq!(state.document.image.annotations().len(), 2);
        assert_eq!(state.editor.selection, None);
        assert_eq!(state.editor.tool, Tool::Pixelate);
        assert!(state.document.image.annotation(existing_id).is_some());
    }

    // -- pixelate direct manipulation (Task 5) -------------------------------

    fn workspace_with_pixelate() -> (super::super::ResultWorkspace, AnnotationId) {
        let mut state = workspace_with_size(200, 200);
        let id = state
            .document
            .image
            .add_pixelate(
                ImageRect {
                    x: 50.0,
                    y: 50.0,
                    width: 40.0,
                    height: 30.0,
                },
                16,
            )
            .unwrap();
        state.editor.tool = Tool::Select;
        state.editor.selection = Some(id);
        (state, id)
    }

    #[test]
    fn select_mode_body_hit_on_pixelate_selects_and_starts_drag() {
        let (mut state, id) = workspace_with_pixelate();
        let _ = handle_canvas_pressed(&mut state, ImagePoint::new(70.0, 65.0), Instant::now());
        assert!(matches!(
            state.editor.drag,
            Some(DragState::EditAnnotation { .. })
        ));
        assert_eq!(state.editor.selection, Some(id));
    }

    #[test]
    fn select_mode_empty_miss_on_pixelate_clears_selection() {
        let (mut state, _id) = workspace_with_pixelate();
        let _ = handle_canvas_pressed(&mut state, ImagePoint::new(10.0, 10.0), Instant::now());
        assert!(state.editor.selection.is_none());
        assert!(matches!(state.editor.drag, Some(DragState::Pan { .. })));
    }

    #[test]
    fn pixelate_body_move_preserves_size_and_block_size() {
        let (mut state, id) = workspace_with_pixelate();
        press_move_release(
            &mut state,
            ImagePoint::new(70.0, 65.0),
            ImagePoint::new(100.0, 95.0),
        );
        match state.document.image.annotation(id).unwrap() {
            Annotation::Pixelate {
                bounds, block_size, ..
            } => {
                assert_eq!(*block_size, 16, "block_size preserved");
                assert_eq!(bounds.width, 40.0, "width preserved");
                assert_eq!(bounds.height, 30.0, "height preserved");
                assert_eq!(bounds.x, 80.0, "x moved");
                assert_eq!(bounds.y, 80.0, "y moved");
            }
            other => panic!("expected Pixelate, got {other:?}"),
        }
    }

    #[test]
    fn pixelate_body_move_clamps_to_image_bounds() {
        let (mut state, id) = workspace_with_pixelate();
        press_move_release(
            &mut state,
            ImagePoint::new(70.0, 65.0),
            ImagePoint::new(200.0, 200.0),
        );
        match state.document.image.annotation(id).unwrap() {
            Annotation::Pixelate { bounds, .. } => {
                assert!(bounds.x + bounds.width <= 200.0);
                assert!(bounds.y + bounds.height <= 200.0);
            }
            other => panic!("expected Pixelate, got {other:?}"),
        }
    }

    #[test]
    fn pixelate_resize_from_handle_commits_new_bounds() {
        let (mut state, id) = workspace_with_pixelate();
        // BottomRight handle at (90, 80)
        press_move_release(
            &mut state,
            ImagePoint::new(90.0, 80.0),
            ImagePoint::new(120.0, 110.0),
        );
        match state.document.image.annotation(id).unwrap() {
            Annotation::Pixelate { bounds, .. } => {
                assert_eq!(bounds.x, 50.0);
                assert_eq!(bounds.y, 50.0);
                assert_eq!(bounds.width, 70.0);
                assert_eq!(bounds.height, 60.0);
            }
            other => panic!("expected Pixelate, got {other:?}"),
        }
    }

    #[test]
    fn pixelate_resize_from_inverted_handle_normalizes() {
        let (mut state, id) = workspace_with_pixelate();
        // Drag BottomRight past TopLeft
        press_move_release(
            &mut state,
            ImagePoint::new(90.0, 80.0),
            ImagePoint::new(30.0, 30.0),
        );
        match state.document.image.annotation(id).unwrap() {
            Annotation::Pixelate { bounds, .. } => {
                assert!(bounds.width > 0.0);
                assert!(bounds.height > 0.0);
                assert_eq!(bounds.x, 30.0);
                assert_eq!(bounds.y, 30.0);
            }
            other => panic!("expected Pixelate, got {other:?}"),
        }
    }

    #[test]
    fn pixelate_resize_with_shift_preserves_aspect_ratio() {
        let (mut state, id) = workspace_with_pixelate();
        let _ = update(
            &mut state,
            Message::ModifiersChanged(iced::keyboard::Modifiers::SHIFT),
        );
        // BottomRight handle at (90, 80)
        press_move_release(
            &mut state,
            ImagePoint::new(90.0, 80.0),
            ImagePoint::new(110.0, 100.0),
        );
        match state.document.image.annotation(id).unwrap() {
            Annotation::Pixelate { bounds, .. } => {
                let ratio = bounds.width / bounds.height;
                assert!(
                    (ratio - 40.0 / 30.0).abs() < 0.01,
                    "aspect ratio preserved: {ratio} vs {}",
                    40.0 / 30.0
                );
            }
            other => panic!("expected Pixelate, got {other:?}"),
        }
    }

    #[test]
    fn pixelate_escape_restores_original_bounds() {
        let (mut state, id) = workspace_with_pixelate();
        let original = state.document.image.annotation(id).unwrap().clone();
        let _ = handle_canvas_pressed(&mut state, ImagePoint::new(90.0, 80.0), Instant::now());
        let _ = handle_canvas_moved(&mut state, ImagePoint::new(120.0, 110.0));
        let _ = update(&mut state, Message::EscapePressed);
        assert!(state.editor.drag.is_none());
        assert_eq!(state.document.image.annotation(id), Some(&original));
    }

    #[test]
    fn pixelate_delete_removes_annotation() {
        let (mut state, _id) = workspace_with_pixelate();
        let _ = update(&mut state, Message::DeleteSelected);
        assert!(state.document.image.annotations().is_empty());
        assert_eq!(state.editor.selection, None);
    }

    #[test]
    fn pixelate_move_or_resize_creates_one_undo_entry() {
        let (mut state, id) = workspace_with_pixelate();
        press_move_release(
            &mut state,
            ImagePoint::new(70.0, 65.0),
            ImagePoint::new(100.0, 95.0),
        );
        assert!(state.document.image.can_undo());
        state.document.image.undo();
        match state.document.image.annotation(id).unwrap() {
            Annotation::Pixelate { bounds, .. } => {
                assert_eq!(bounds.x, 50.0);
                assert_eq!(bounds.y, 50.0);
            }
            other => panic!("expected Pixelate, got {other:?}"),
        }
    }

    #[test]
    fn pixelate_tool_creates_over_selected_pixelate() {
        let mut state = workspace_with_size(200, 200);
        let id = state
            .document
            .image
            .add_pixelate(
                ImageRect {
                    x: 50.0,
                    y: 50.0,
                    width: 40.0,
                    height: 30.0,
                },
                16,
            )
            .unwrap();
        state.editor.tool = Tool::Pixelate;
        state.editor.selection = Some(id);

        let original = state.document.image.annotation(id).unwrap().clone();

        let _ = handle_canvas_pressed(&mut state, ImagePoint::new(70.0, 65.0), Instant::now());
        assert!(matches!(
            state.editor.drag,
            Some(DragState::CreatePixelate { .. })
        ));
        let _ = handle_canvas_moved(&mut state, ImagePoint::new(110.0, 105.0));
        let _ = handle_canvas_released(&mut state, ImagePoint::new(110.0, 105.0));

        assert_eq!(state.document.image.annotation(id), Some(&original));
        assert_eq!(state.document.image.annotations().len(), 2);
    }

    #[test]
    fn apply_opacity_to_selected_pen_freehand_does_nothing() {
        let mut state = workspace();
        let id = state
            .document
            .image
            .add_freehand_with_style(
                rollshot_image_document::FreehandKind::Pen,
                vec![ImagePoint::new(0.0, 0.0), ImagePoint::new(10.0, 10.0)],
                StrokeStyle::default(),
            )
            .unwrap();
        state.editor.tool = Tool::Select;
        state.editor.selection = Some(id);
        // PreviewStrokeOpacity targets Highlighter only; Pen selection → no-op
        let _ = update(&mut state, Message::PreviewStrokeOpacity(0.5));
        assert!(state.editor.properties.opacity.is_none());
    }

    // -- pixelate cache-isolation output tests (Task 8 review finding) -------

    /// Build a workspace with a gradient image so pixelate produces visible
    /// differences (uniform-color images hide pixelate effects).
    fn workspace_with_gradient(w: u32, h: u32) -> super::super::ResultWorkspace {
        let mut img = RgbaImage::new(w, h);
        for y in 0..h {
            for x in 0..w {
                img.put_pixel(
                    x,
                    y,
                    Rgba([(x % 256) as u8, (y % 256) as u8, ((x + y) % 256) as u8, 255]),
                );
            }
        }
        let mut ws = super::super::ResultWorkspace::with_max_texture_dim(
            super::super::document::ResultDocument::unsaved(img),
            None,
            super::super::DEFAULT_MAX_TEXTURE_DIM,
        );
        ws.viewport.zoom = ZoomMode::ActualSize;
        ws.apply_viewport_bounds(Size::new(w as f32, h as f32));
        ws
    }

    #[test]
    fn pixelate_committed_appear_in_copy_and_save_with_empty_preview_cache() {
        let mut state = workspace_with_gradient(200, 200);
        state
            .document
            .image
            .add_pixelate(
                ImageRect {
                    x: 10.0,
                    y: 10.0,
                    width: 60.0,
                    height: 60.0,
                },
                16,
            )
            .unwrap();
        // Cache is empty (fresh workspace). Output must still contain the
        // committed pixelate at full resolution via flatten, not the cache.
        let source = state.document.image.source();
        let copy = copy_payload(&state);
        let save = save_payload(&state);
        let flattened = state.document.image.flatten();

        assert_eq!(copy, flattened, "copy must equal flatten");
        assert_eq!(save, flattened, "save must equal flatten");
        assert_ne!(
            copy.get_pixel(40, 40),
            source.get_pixel(40, 40),
            "committed pixelate region must differ from source in output"
        );
    }

    #[test]
    fn pixelate_committed_appear_with_in_flight_preview_cache() {
        let mut state = workspace_with_gradient(200, 200);
        state
            .document
            .image
            .add_pixelate(
                ImageRect {
                    x: 10.0,
                    y: 10.0,
                    width: 60.0,
                    height: 60.0,
                },
                16,
            )
            .unwrap();
        // Put a request in-flight (simulates a pending async preview).
        let source = state.document.image.shared_source();
        let source_id = super::super::pixelate_preview::source_id_from_arc(&source);
        let region = rollshot_image_document::pixelate::RasterRegion {
            x: 10,
            y: 10,
            width: 60,
            height: 60,
        };
        let key = super::super::pixelate_preview::PreviewKey::new(source_id, region, 16, 1.0);
        let _request = state.pixelate_previews.begin_request(key);
        assert!(
            state.pixelate_previews.is_in_flight(key),
            "precondition: key is in-flight"
        );

        let flattened = state.document.image.flatten();
        let copy = copy_payload(&state);
        assert_eq!(copy, flattened, "in-flight cache must not affect output");
    }

    #[test]
    fn pixelate_committed_appear_with_failed_preview_cache() {
        let mut state = workspace_with_gradient(200, 200);
        state
            .document
            .image
            .add_pixelate(
                ImageRect {
                    x: 10.0,
                    y: 10.0,
                    width: 60.0,
                    height: 60.0,
                },
                16,
            )
            .unwrap();
        // Simulate a failed preview request.
        let source = state.document.image.shared_source();
        let source_id = super::super::pixelate_preview::source_id_from_arc(&source);
        let region = rollshot_image_document::pixelate::RasterRegion {
            x: 10,
            y: 10,
            width: 60,
            height: 60,
        };
        let key = super::super::pixelate_preview::PreviewKey::new(source_id, region, 16, 1.0);
        let request = state.pixelate_previews.begin_request(key).unwrap();
        state.pixelate_previews.fail(request);

        let flattened = state.document.image.flatten();
        let copy = copy_payload(&state);
        assert_eq!(copy, flattened, "failed cache must not affect output");
    }

    #[test]
    fn copy_original_is_byte_identical_to_source() {
        let mut state = workspace_with_gradient(200, 200);
        state
            .document
            .image
            .add_pixelate(
                ImageRect {
                    x: 10.0,
                    y: 10.0,
                    width: 60.0,
                    height: 60.0,
                },
                16,
            )
            .unwrap();
        let original = copy_original_payload(&state);
        assert_eq!(
            original.as_raw(),
            state.document.image.source().as_raw(),
            "Copy Original must return source bytes unchanged"
        );
    }

    #[test]
    fn pixelate_draft_and_property_preview_never_enter_output() {
        let mut state = workspace_with_gradient(200, 200);
        // Commit one pixelate so we have a selection target.
        let id = state
            .document
            .image
            .add_pixelate(
                ImageRect {
                    x: 10.0,
                    y: 10.0,
                    width: 40.0,
                    height: 30.0,
                },
                16,
            )
            .unwrap();
        state.editor.tool = Tool::Select;
        state.editor.selection = Some(id);

        // Start an in-progress pixelate creation (draft, not committed).
        state.editor.tool = Tool::Pixelate;
        let _ = handle_canvas_pressed(&mut state, ImagePoint::new(150.0, 150.0), Instant::now());
        let _ = handle_canvas_moved(&mut state, ImagePoint::new(180.0, 180.0));
        assert!(matches!(
            state.editor.drag,
            Some(DragState::CreatePixelate { .. })
        ));

        // Also start a block-size property preview.
        let _ = update(&mut state, Message::PreviewPixelateBlockSize(32));

        let source = state.document.image.source();
        let output = copy_payload(&state);
        let flattened = state.document.image.flatten();
        assert_eq!(output, flattened, "copy equals flatten");

        // Draft center (165, 165) must NOT appear in output.
        assert_eq!(
            output.get_pixel(165, 165),
            source.get_pixel(165, 165),
            "uncommitted pixelate draft must stay out of output"
        );
        // Selection handle area (5, 5) must NOT appear in output.
        assert_eq!(
            output.get_pixel(5, 5),
            source.get_pixel(5, 5),
            "selection handle area must stay out of output"
        );
    }

    #[test]
    fn pixelate_block_size_editing_changes_output_by_resampling() {
        let mut state = workspace_with_gradient(200, 200);
        let id = state
            .document
            .image
            .add_pixelate(
                ImageRect {
                    x: 10.0,
                    y: 10.0,
                    width: 60.0,
                    height: 60.0,
                },
                16,
            )
            .unwrap();
        let output_bs16 = state.document.image.flatten();

        // Change block_size — this must produce a different flatten.
        state
            .document
            .image
            .set_pixelate_block_size(id, 32)
            .unwrap();
        let output_bs32 = state.document.image.flatten();

        assert_ne!(
            output_bs16, output_bs32,
            "changing block_size must change flatten output"
        );
        // The source must still be immutable.
        let source = state.document.image.source();
        assert_eq!(
            source.get_pixel(0, 0).0,
            [0, 0, 0, 255],
            "source bytes unchanged"
        );
    }

    #[test]
    fn pixelate_move_changes_output_by_resampling() {
        let mut state = workspace_with_gradient(200, 200);
        let id = state
            .document
            .image
            .add_pixelate(
                ImageRect {
                    x: 10.0,
                    y: 10.0,
                    width: 60.0,
                    height: 60.0,
                },
                16,
            )
            .unwrap();
        let output_before = state.document.image.flatten();

        // Move the pixelate annotation.
        state
            .document
            .image
            .set_pixelate_bounds(
                id,
                ImageRect {
                    x: 80.0,
                    y: 80.0,
                    width: 60.0,
                    height: 60.0,
                },
            )
            .unwrap();
        let output_after = state.document.image.flatten();

        assert_ne!(
            output_before, output_after,
            "moving pixelate must change flatten output"
        );
    }

    #[test]
    fn pixelate_resize_changes_output_by_resampling() {
        let mut state = workspace_with_gradient(200, 200);
        let id = state
            .document
            .image
            .add_pixelate(
                ImageRect {
                    x: 10.0,
                    y: 10.0,
                    width: 40.0,
                    height: 30.0,
                },
                16,
            )
            .unwrap();
        let output_before = state.document.image.flatten();

        // Resize the pixelate annotation.
        state
            .document
            .image
            .set_pixelate_bounds(
                id,
                ImageRect {
                    x: 10.0,
                    y: 10.0,
                    width: 100.0,
                    height: 80.0,
                },
            )
            .unwrap();
        let output_after = state.document.image.flatten();

        assert_ne!(
            output_before, output_after,
            "resizing pixelate must change flatten output"
        );
    }

    #[test]
    fn repeated_flatten_leaves_source_bytes_unchanged() {
        let mut state = workspace_with_gradient(200, 200);
        state
            .document
            .image
            .add_pixelate(
                ImageRect {
                    x: 10.0,
                    y: 10.0,
                    width: 60.0,
                    height: 60.0,
                },
                16,
            )
            .unwrap();
        let source_before = state.document.image.source().as_raw().to_vec();

        // Flatten multiple times.
        let _ = state.document.image.flatten();
        let _ = state.document.image.flatten();
        let _ = state.document.image.flatten();

        let source_after = state.document.image.source().as_raw().to_vec();
        assert_eq!(
            source_before, source_after,
            "repeated flatten must not mutate source bytes"
        );
    }

    // -- imported source protection (Task 4) ----------------------------------

    fn imported_workspace_for_save() -> (tempfile::TempDir, super::super::ResultWorkspace, PathBuf)
    {
        let dir = tempfile::tempdir().unwrap();
        let source_path = dir.path().join("source.png");
        image::RgbaImage::new(4, 4)
            .save_with_format(&source_path, image::ImageFormat::Png)
            .unwrap();
        let imported = crate::image_import::load(&source_path).unwrap();
        let state = super::super::ResultWorkspace::with_config_path(
            super::super::document::ResultDocument::imported(imported.pixels, imported.source),
            None,
            None,
        );
        (dir, state, source_path)
    }

    #[test]
    fn rejected_imported_destinations_preserve_document_and_export_state() {
        let (dir, mut state, source_path) = imported_workspace_for_save();
        for rejected_path in [source_path, dir.path().join("wrong-extension.jpg")] {
            let before_state_id = state.document.image.state_id();
            let before_export = state.document.last_export_path.clone();
            let task = update(&mut state, Message::SavePathChosen(Some(rejected_path)));
            drop(task);
            assert_eq!(state.document.image.state_id(), before_state_id);
            assert_eq!(state.document.last_export_path, before_export);
            assert!(state.message.as_ref().unwrap().is_error());
        }
    }

    #[test]
    fn save_to_unverifiable_destination_is_rejected_before_write() {
        let (dir, mut state, _source_path) = imported_workspace_for_save();
        let unverifiable = dir.path().join("missing-parent").join("export.png");
        let before_state_id = state.document.image.state_id();
        let before_export = state.document.last_export_path.clone();

        let task = update(&mut state, Message::SavePathChosen(Some(unverifiable)));
        drop(task);

        assert_eq!(state.document.image.state_id(), before_state_id);
        assert_eq!(state.document.last_export_path, before_export);
        assert_eq!(
            state.message.as_ref().map(|message| message.text()),
            Some(super::super::secure_sharing::DESTINATION_VERIFICATION_ERROR)
        );
    }
}
