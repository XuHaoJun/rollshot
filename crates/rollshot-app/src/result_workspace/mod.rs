pub mod actions;
pub mod viewport;

use iced::widget::image::Handle as ImageHandle;
use iced::widget::{
    button, column, container, image as image_widget, mouse_area, row, scrollable, text, Space,
};
use iced::{keyboard, mouse, Alignment, Element, Length, Point, Size, Subscription, Task, Vector};
use image::RgbaImage;
use std::path::PathBuf;
use std::time::{Duration, Instant};
use viewport::{
    anchored_scroll, display_downscale_scale, geometry_for, step_zoom, ViewportState,
    ZoomDirection, ZoomMode, DEFAULT_MAX_TEXTURE_DIM,
};

const SUCCESS_MESSAGE_DURATION: Duration = Duration::from_secs(4);
const UNSAVED_LABEL: &str = "Unsaved capture";
const DISCARD_PROMPT: &str = "Discard unsaved capture?";
const SCROLLBAR_WIDTH: f32 = 14.0;
const SCROLLBAR_SPACING: f32 = 2.0;
/// Pixel step for a single wheel "line" of scrolling.
const WHEEL_LINE_PX: f32 = 60.0;

// ---------------------------------------------------------------------------
// Document
// ---------------------------------------------------------------------------

pub struct ResultDocument {
    pub source_image: RgbaImage,
    pub saved_path: Option<PathBuf>,
}

impl ResultDocument {
    pub fn saved(image: RgbaImage, path: PathBuf) -> Self {
        Self {
            source_image: image,
            saved_path: Some(path),
        }
    }

    pub fn unsaved(image: RgbaImage) -> Self {
        Self {
            source_image: image,
            saved_path: None,
        }
    }
}

// ---------------------------------------------------------------------------
// Inline message
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InlineMessage {
    Success { text: String, expires_at: Instant },
    Error(String),
}

impl InlineMessage {
    pub fn text(&self) -> &str {
        match self {
            InlineMessage::Success { text, .. } => text,
            InlineMessage::Error(text) => text,
        }
    }

    fn success(text: String) -> Self {
        InlineMessage::Success {
            text,
            expires_at: Instant::now() + SUCCESS_MESSAGE_DURATION,
        }
    }

    fn is_error(&self) -> bool {
        matches!(self, InlineMessage::Error(_))
    }

    fn expiry(&self) -> Option<Instant> {
        match self {
            InlineMessage::Success { expires_at, .. } => Some(*expires_at),
            InlineMessage::Error(_) => None,
        }
    }
}

// ---------------------------------------------------------------------------
// Close decision
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloseDecision {
    Close,
    ConfirmDiscard,
}

pub fn close_decision(document: &ResultDocument) -> CloseDecision {
    if document.saved_path.is_some() {
        CloseDecision::Close
    } else {
        CloseDecision::ConfirmDiscard
    }
}

// ---------------------------------------------------------------------------
// Workspace
// ---------------------------------------------------------------------------

pub struct ResultWorkspace {
    pub document: ResultDocument,
    pub message: Option<InlineMessage>,
    pub confirming_discard: bool,
    /// Iced image handle built once. For oversized captures this is a
    /// downscaled display copy (spec §9.6); the document keeps the full source.
    pub image_handle: ImageHandle,
    /// Current zoom mode + scroll offset state.
    pub viewport: ViewportState,
    /// Current keyboard modifiers, tracked for wheel routing.
    pub modifiers: keyboard::Modifiers,
    /// Last known canvas pointer position (scrollable-local, for
    /// pointer-anchored zoom). Fed solely by the canvas `mouse_area.on_move`.
    pub pointer_position: Point,
    /// Last reported scrollable bounds (visible canvas area).
    pub viewport_bounds: Size,
    /// Identity of the single canvas scrollable, for scroll operations.
    pub scrollable_id: iced::widget::Id,
}

impl ResultWorkspace {
    pub fn new(document: ResultDocument, initial_error: Option<String>) -> Self {
        Self::with_max_texture_dim(document, initial_error, DEFAULT_MAX_TEXTURE_DIM)
    }

    /// Construct with an explicit texture ceiling (used by tests; production
    /// uses [`DEFAULT_MAX_TEXTURE_DIM`]).
    pub fn with_max_texture_dim(
        document: ResultDocument,
        initial_error: Option<String>,
        max_texture_dim: u32,
    ) -> Self {
        let source_size = Size::new(
            document.source_image.width() as f32,
            document.source_image.height() as f32,
        );
        let scale = display_downscale_scale(source_size, max_texture_dim);
        let image_handle = build_display_handle(&document.source_image, scale);

        let message = if let Some(err) = initial_error {
            Some(InlineMessage::Error(err))
        } else {
            document
                .saved_path
                .as_deref()
                .map(|path| InlineMessage::success(format!("Saved to {}", path.display())))
        };

        let zoom = viewport::default_zoom(source_size);

        Self {
            document,
            message,
            confirming_discard: false,
            image_handle,
            viewport: ViewportState {
                zoom,
                scroll_offset: Vector::new(0.0, 0.0),
            },
            modifiers: keyboard::Modifiers::default(),
            pointer_position: Point::ORIGIN,
            viewport_bounds: Size::new(1.0, 1.0),
            scrollable_id: iced::widget::Id::unique(),
        }
    }

    // Used by the macOS product daemon (Task 8); no Linux caller, so it stays
    // dead-code-allowed only off macOS to keep Linux clippy clean.
    #[allow(dead_code)]
    pub fn message_text(&self) -> Option<String> {
        self.message.as_ref().map(|m| m.text().to_owned())
    }

    /// Reveal is only meaningful once the capture has a saved path on disk.
    pub fn can_reveal(&self) -> bool {
        self.document.saved_path.is_some()
    }

    /// Original (full-resolution) image dimensions, reported by the status bar
    /// regardless of any display downscale.
    fn original_size(&self) -> Size {
        Size::new(
            self.document.source_image.width() as f32,
            self.document.source_image.height() as f32,
        )
    }

    /// Record the latest scrollable bounds so fit modes and pointer anchoring
    /// use the real visible canvas area. Fit modes recompute on the next render;
    /// Custom / ActualSize keep their percentage (handled by the zoom math).
    pub fn apply_viewport_bounds(&mut self, bounds: Size) {
        self.viewport_bounds = bounds;
    }

    /// Apply the result of a save-as dialog + write.
    ///
    /// - `Ok(Some(path))` — user chose a path and the write succeeded.
    /// - `Ok(None)` — user cancelled the dialog; no change.
    /// - `Err(e)` — write failed; show a persistent error.
    pub fn apply_save_as(&mut self, result: Result<Option<PathBuf>, String>) {
        match result {
            Ok(Some(path)) => {
                let text = format!("Saved to {}", path.display());
                self.document.saved_path = Some(path);
                self.message = Some(InlineMessage::success(text));
                // A successful save resolves the unsaved state, so any pending
                // discard prompt should close.
                self.confirming_discard = false;
            }
            Ok(None) => {
                // User cancelled — no change, no error.
            }
            Err(e) => {
                self.message = Some(InlineMessage::Error(e));
            }
        }
    }
}

/// Build an iced image handle from the source, downscaling when `scale < 1.0`
/// (spec §9.6). `scale == 1.0` uses the full-resolution pixels directly.
fn build_display_handle(source: &RgbaImage, scale: f32) -> ImageHandle {
    if scale >= 1.0 {
        return ImageHandle::from_rgba(source.width(), source.height(), source.as_raw().clone());
    }
    let w = ((source.width() as f32 * scale).round() as u32).max(1);
    let h = ((source.height() as f32 * scale).round() as u32).max(1);
    let resized = image::imageops::resize(source, w, h, image::imageops::FilterType::Triangle);
    ImageHandle::from_rgba(w, h, resized.into_raw())
}

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
}

// ---------------------------------------------------------------------------
// Update
// ---------------------------------------------------------------------------

pub(crate) fn update(state: &mut ResultWorkspace, message: Message) -> Task<Message> {
    match message {
        Message::RequestClose => match close_decision(&state.document) {
            CloseDecision::Close => iced::exit(),
            CloseDecision::ConfirmDiscard => {
                state.confirming_discard = true;
                Task::none()
            }
        },
        Message::ConfirmDiscard => iced::exit(),
        Message::KeepUnsaved => {
            state.confirming_discard = false;
            Task::none()
        }
        Message::DismissMessage => {
            state.message = None;
            Task::none()
        }
        Message::Copy => {
            let result = actions::copy_image(&state.document.source_image);
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
            let default_dir = crate::storage::Platform::current()
                .and_then(crate::storage::default_output_dir)
                .unwrap_or_else(|_| PathBuf::from("."));
            let default_name = default_save_name(&state.document);
            Task::perform(
                actions::prompt_save_as(default_dir, default_name),
                Message::SavePathChosen,
            )
        }
        Message::SavePathChosen(Some(path)) => {
            // Clone the source so the write future owns its pixels.
            let image = state.document.source_image.clone();
            Task::perform(
                async move { actions::write_save_as(&image, &path) },
                Message::SaveFinished,
            )
        }
        Message::SavePathChosen(None) => Task::none(),
        Message::SaveFinished(result) => {
            state.apply_save_as(result.map(Some));
            Task::none()
        }
        Message::Reveal => {
            let Some(path) = state.document.saved_path.clone() else {
                return Task::none();
            };
            Task::done(Message::RevealFinished(actions::reveal(&path)))
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
fn apply_zoom_at_pointer(state: &mut ResultWorkspace, next: ZoomMode) -> Task<Message> {
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
fn handle_wheel(state: &mut ResultWorkspace, delta: mouse::ScrollDelta) -> Task<Message> {
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

fn default_save_name(document: &ResultDocument) -> String {
    document
        .saved_path
        .as_ref()
        .and_then(|p| p.file_name())
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| {
            let timestamp = chrono::Local::now().format("%Y-%m-%d %H.%M.%S");
            format!("Rollshot {timestamp}.png")
        })
}

// ---------------------------------------------------------------------------
// View
// ---------------------------------------------------------------------------

pub(crate) fn view(state: &ResultWorkspace) -> Element<'_, Message> {
    let original = state.original_size();

    let title = state
        .document
        .saved_path
        .as_ref()
        .and_then(|p| p.file_name())
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| UNSAVED_LABEL.to_string());

    let toolbar = row![
        button(text("Close")).on_press(Message::RequestClose),
        text(title).width(Length::Fill),
        button(text("Copy")).on_press(Message::Copy),
        button(text("Save As")).on_press(Message::SaveAs),
        reveal_button(state),
    ]
    .spacing(8)
    .align_y(Alignment::Center);

    let message_area = message_row(state);

    let canvas = canvas_view(state, original);

    let status = status_bar(state, original);

    let layout = column![toolbar, message_area, canvas, status]
        .spacing(8)
        .padding(8);

    if state.confirming_discard {
        discard_modal(layout)
    } else {
        layout.into()
    }
}

fn reveal_button(state: &ResultWorkspace) -> Element<'_, Message> {
    let btn = button(text("Reveal"));
    if state.can_reveal() {
        btn.on_press(Message::Reveal).into()
    } else {
        btn.into()
    }
}

fn message_row(state: &ResultWorkspace) -> Element<'_, Message> {
    match &state.message {
        Some(msg) => {
            let mut content = row![text(msg.text().to_owned()).width(Length::Fill)]
                .spacing(8)
                .align_y(Alignment::Center);
            if msg.is_error() {
                content = content.push(button(text("Dismiss")).on_press(Message::DismissMessage));
            }
            container(content).width(Length::Fill).padding(4).into()
        }
        None => Space::new()
            .width(Length::Fill)
            .height(Length::Fixed(0.0))
            .into(),
    }
}

fn canvas_view<'a>(state: &'a ResultWorkspace, image_size: Size) -> Element<'a, Message> {
    let geometry = geometry_for(state.viewport.zoom, image_size, state.viewport_bounds);

    let img = image_widget(state.image_handle.clone())
        .width(Length::Fixed(geometry.rendered_size.width))
        .height(Length::Fixed(geometry.rendered_size.height));

    // Place the (possibly centered) image inside content sized to the geometry.
    let content = container(img)
        .width(Length::Fixed(geometry.content_size.width))
        .height(Length::Fixed(geometry.content_size.height))
        .padding(iced::Padding {
            left: geometry.image_origin.x,
            top: geometry.image_origin.y,
            right: 0.0,
            bottom: 0.0,
        });

    let vertical = if geometry.vertical_overflow {
        thick_scrollbar()
    } else {
        scrollable::Scrollbar::hidden()
    };
    let horizontal = if geometry.horizontal_overflow {
        thick_scrollbar()
    } else {
        scrollable::Scrollbar::hidden()
    };

    let scroller = scrollable(content)
        .direction(scrollable::Direction::Both {
            vertical,
            horizontal,
        })
        .id(state.scrollable_id.clone())
        .on_scroll(|viewport| {
            let off = viewport.absolute_offset();
            let bounds = viewport.bounds();
            Message::ViewportChanged {
                bounds: bounds.size(),
                offset: Vector::new(off.x, off.y),
            }
        })
        .width(Length::Fill)
        .height(Length::Fill);

    mouse_area(scroller)
        .on_move(Message::PointerMoved)
        .on_scroll(Message::WheelScrolled)
        .into()
}

fn thick_scrollbar() -> scrollable::Scrollbar {
    scrollable::Scrollbar::new()
        .width(SCROLLBAR_WIDTH)
        .scroller_width(SCROLLBAR_WIDTH)
        .spacing(SCROLLBAR_SPACING)
}

fn status_bar(state: &ResultWorkspace, image_size: Size) -> Element<'_, Message> {
    let dims = format!("{} × {}", image_size.width as u32, image_size.height as u32);
    let zoom_label = zoom_label(state);

    row![
        text(dims),
        text(zoom_label).width(Length::Fill),
        button(text("Fit Width")).on_press(Message::SetZoom(ZoomMode::FitWidth)),
        button(text("Fit Window")).on_press(Message::SetZoom(ZoomMode::FitWindow)),
        button(text("Fit Height")).on_press(Message::SetZoom(ZoomMode::FitHeight)),
        button(text("100%")).on_press(Message::SetZoom(ZoomMode::ActualSize)),
        button(text("-")).on_press(Message::ZoomStep(ZoomDirection::Out)),
        button(text("+")).on_press(Message::ZoomStep(ZoomDirection::In)),
    ]
    .spacing(8)
    .align_y(Alignment::Center)
    .into()
}

fn zoom_label(state: &ResultWorkspace) -> String {
    match state.viewport.zoom {
        ZoomMode::FitWidth => "Fit Width".to_string(),
        ZoomMode::FitWindow => "Fit Window".to_string(),
        ZoomMode::FitHeight => "Fit Height".to_string(),
        ZoomMode::ActualSize => "100%".to_string(),
        ZoomMode::Custom(p) => format!("{p}%"),
    }
}

fn discard_modal(base: iced::widget::Column<'_, Message>) -> Element<'_, Message> {
    let dialog = container(
        column![
            text(DISCARD_PROMPT),
            row![
                button(text("Keep")).on_press(Message::KeepUnsaved),
                button(text("Discard")).on_press(Message::ConfirmDiscard),
            ]
            .spacing(8),
        ]
        .spacing(12)
        .align_x(Alignment::Center),
    )
    .padding(20);

    // Full-window scrim that actually blocks the base layer. iced's `stack`
    // only levitates the cursor (suppressing input to lower layers) where the
    // top layer reports a non-`None` `mouse_interaction`. A bare centered
    // container leaves the surrounding area at `Interaction::None`, so toolbar
    // buttons behind it stay clickable. Setting `mouse_area.interaction(Idle)`
    // makes the scrim report a non-`None` interaction over the whole window,
    // and `on_press` captures clicks outside the dialog (mapped to a no-op so
    // an accidental outside-click neither dismisses nor discards).
    let scrim = mouse_area(
        container(dialog)
            .width(Length::Fill)
            .height(Length::Fill)
            .align_x(Alignment::Center)
            .align_y(Alignment::Center),
    )
    .interaction(mouse::Interaction::Idle)
    .on_press(Message::ModalScrimPressed);

    iced::widget::stack![base, scrim].into()
}

// ---------------------------------------------------------------------------
// Subscription
// ---------------------------------------------------------------------------

pub(crate) fn subscription(state: &ResultWorkspace) -> Subscription<Message> {
    let mut subs = vec![
        iced::window::close_requests().map(|_id| Message::RequestClose),
        iced::event::listen_with(|event, _status, _window| match event {
            iced::Event::Keyboard(keyboard::Event::ModifiersChanged(m)) => {
                Some(Message::ModifiersChanged(m))
            }
            iced::Event::Keyboard(keyboard::Event::KeyPressed {
                key: keyboard::Key::Named(keyboard::key::Named::Escape),
                ..
            }) => Some(Message::RequestClose),
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
// Runner (Linux standalone window)
// ---------------------------------------------------------------------------

#[cfg(target_os = "linux")]
pub fn run(document: ResultDocument, initial_error: Option<String>) -> Result<(), String> {
    use std::sync::{Arc, Mutex};

    let boot_data = Arc::new(Mutex::new(Some((document, initial_error))));

    let boot = move || {
        let (document, initial_error) = boot_data
            .lock()
            .unwrap()
            .take()
            .expect("result workspace boot data already consumed");
        (ResultWorkspace::new(document, initial_error), Task::none())
    };

    iced::application(boot, update, view)
        .title("Rollshot")
        .subscription(subscription)
        .window(iced::window::Settings {
            size: Size::new(1100.0, 760.0),
            min_size: Some(Size::new(640.0, 420.0)),
            decorations: true,
            resizable: true,
            exit_on_close_request: false,
            ..Default::default()
        })
        .run()
        .map_err(|e| e.to_string())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgba;
    use std::path::Path;

    fn image() -> RgbaImage {
        RgbaImage::from_pixel(2, 2, Rgba([100, 150, 200, 255]))
    }

    fn workspace() -> ResultWorkspace {
        ResultWorkspace::new(ResultDocument::unsaved(image()), None)
    }

    fn unsaved_workspace() -> ResultWorkspace {
        ResultWorkspace::new(ResultDocument::unsaved(image()), None)
    }

    fn saved_workspace() -> ResultWorkspace {
        ResultWorkspace::new(
            ResultDocument::saved(image(), PathBuf::from("/tmp/result.png")),
            None,
        )
    }

    // -- existing model tests (Task 3/4) -------------------------------------

    #[test]
    fn saved_document_closes_immediately() {
        let document = ResultDocument::saved(image(), PathBuf::from("/tmp/result.png"));
        assert_eq!(close_decision(&document), CloseDecision::Close);
    }

    #[test]
    fn unsaved_document_requests_discard_confirmation() {
        let document = ResultDocument::unsaved(image());
        assert_eq!(close_decision(&document), CloseDecision::ConfirmDiscard);
    }

    #[test]
    fn save_as_success_updates_saved_path_and_message() {
        let mut state = workspace();
        state.apply_save_as(Ok(Some(PathBuf::from("/tmp/result.png"))));
        assert_eq!(
            state.document.saved_path.as_deref(),
            Some(Path::new("/tmp/result.png"))
        );
        assert!(matches!(state.message, Some(InlineMessage::Success { .. })));
    }

    #[test]
    fn saved_workspace_starts_with_saved_path_message() {
        let path = PathBuf::from("/tmp/result.png");
        let state = ResultWorkspace::new(ResultDocument::saved(image(), path.clone()), None);
        assert_eq!(
            state.message_text(),
            Some(format!("Saved to {}", path.display()))
        );
    }

    #[test]
    fn unsaved_workspace_with_initial_error_has_error_message() {
        let err = "disk full".to_string();
        let state = ResultWorkspace::new(ResultDocument::unsaved(image()), Some(err.clone()));
        assert!(matches!(&state.message, Some(InlineMessage::Error(e)) if e == &err));
    }

    #[test]
    fn save_as_cancel_leaves_no_change() {
        let mut state = workspace();
        state.apply_save_as(Ok(None));
        assert!(state.document.saved_path.is_none());
        assert!(state.message.is_none());
    }

    #[test]
    fn save_as_error_sets_persistent_error_and_no_path() {
        let mut state = workspace();
        state.apply_save_as(Err("write failed".to_string()));
        assert!(state.document.saved_path.is_none());
        assert!(matches!(&state.message, Some(InlineMessage::Error(e)) if e == "write failed"));
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

    // -- resize behavior -----------------------------------------------------

    #[test]
    fn resize_keeps_custom_zoom_percentage() {
        let mut state = workspace();
        state.viewport.zoom = ZoomMode::Custom(150);
        state.apply_viewport_bounds(Size::new(900.0, 700.0));
        assert_eq!(state.viewport.zoom, ZoomMode::Custom(150));
    }

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
        assert!(state.confirming_discard);
    }

    #[test]
    fn saved_close_does_not_confirm_discard() {
        let mut state = saved_workspace();
        let _ = update(&mut state, Message::RequestClose);
        assert!(!state.confirming_discard);
    }

    #[test]
    fn confirm_discard_then_keep_unsaved_transitions() {
        let mut state = unsaved_workspace();
        let _ = update(&mut state, Message::RequestClose);
        assert!(state.confirming_discard);
        let _ = update(&mut state, Message::KeepUnsaved);
        assert!(!state.confirming_discard);
    }

    // -- reveal gating -------------------------------------------------------

    #[test]
    fn reveal_is_disabled_without_a_saved_path() {
        assert!(!unsaved_workspace().can_reveal());
        assert!(saved_workspace().can_reveal());
    }

    #[test]
    fn can_reveal_toggles_after_successful_save_as() {
        let mut state = unsaved_workspace();
        assert!(!state.can_reveal());
        state.apply_save_as(Ok(Some(PathBuf::from("/tmp/result.png"))));
        assert!(state.can_reveal());
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

    // -- §9.6 display downscale ----------------------------------------------

    #[test]
    fn small_image_uses_full_resolution_and_reports_original_dims() {
        let img = RgbaImage::from_pixel(640, 480, Rgba([1, 2, 3, 255]));
        let state = ResultWorkspace::with_max_texture_dim(ResultDocument::unsaved(img), None, 8192);
        // Original dimensions reported regardless of display copy.
        assert_eq!(state.original_size(), Size::new(640.0, 480.0));
    }

    #[test]
    fn oversized_image_keeps_full_res_source_and_original_dims() {
        // Long capture: height exceeds a small ceiling so the display copy is
        // downscaled, but the source + reported dims stay original.
        let img = RgbaImage::from_pixel(100, 400, Rgba([9, 9, 9, 255]));
        let state = ResultWorkspace::with_max_texture_dim(ResultDocument::unsaved(img), None, 200);
        assert_eq!(state.document.source_image.dimensions(), (100, 400));
        assert_eq!(state.original_size(), Size::new(100.0, 400.0));
    }

    /// Both axes over the ceiling: the DISPLAY handle is downscaled so both
    /// display dims land at/under the ceiling, while the source + reported
    /// original dims stay at the full resolution.
    #[test]
    fn oversized_both_axes_downscales_display_handle_only() {
        let ceiling = 8192u32;
        let img = RgbaImage::from_pixel(16000, 10000, Rgba([7, 7, 7, 255]));
        let state =
            ResultWorkspace::with_max_texture_dim(ResultDocument::unsaved(img), None, ceiling);

        // Source + status-bar dims stay full resolution.
        assert_eq!(state.document.source_image.dimensions(), (16000, 10000));
        assert_eq!(state.original_size(), Size::new(16000.0, 10000.0));

        // Display handle was downscaled: both axes at/under the ceiling.
        let (dw, dh) = match state.image_handle.clone() {
            ImageHandle::Rgba { width, height, .. } => (width, height),
            _ => panic!("expected an rgba display handle"),
        };
        assert!(dw <= ceiling, "display width {dw} should be <= {ceiling}");
        assert!(dh <= ceiling, "display height {dh} should be <= {ceiling}");
        // The longest axis (16000) drives the scale, so the display copy is
        // strictly smaller than the source on that axis.
        assert!(dw < 16000, "display width {dw} should be downscaled");
    }

    // -- wheel routing (Task 5 follow-up) ------------------------------------

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

    #[test]
    fn wheel_with_zoom_modifier_zooms_and_leaves_scroll_routing() {
        let mut state = workspace();
        // Give the canvas a real viewport + a baseline custom zoom so the
        // zoom branch produces an observable stepped change.
        state.apply_viewport_bounds(Size::new(800.0, 600.0));
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

    // -- discard modal / save interaction ------------------------------------

    #[test]
    fn save_as_success_clears_pending_discard_prompt() {
        let mut state = unsaved_workspace();
        let _ = update(&mut state, Message::RequestClose);
        assert!(state.confirming_discard, "unsaved close should prompt");
        state.apply_save_as(Ok(Some(PathBuf::from("/tmp/result.png"))));
        assert!(
            !state.confirming_discard,
            "a successful save should close the discard prompt"
        );
    }
}
