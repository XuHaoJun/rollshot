pub mod actions;
pub(crate) mod annotation_defaults;
pub(crate) mod canvas;
mod document;
mod navigator;
#[cfg(feature = "ocr")]
pub(crate) mod ocr_layer;
#[cfg(feature = "ocr")]
pub(crate) mod ocr_text;
pub(crate) mod properties;
mod secure_sharing;
pub(crate) mod toolbar;
mod update;
mod view;
pub mod viewport;
pub mod workbench;

#[allow(unused_imports)]
pub use document::{close_decision, CloseDecision, DiscardPrompt, ResultDocument};
pub use update::Message;
pub(crate) use update::{subscription, update};
pub(crate) use view::view;

use iced::widget::image::Handle as ImageHandle;
use image::RgbaImage;
use std::path::PathBuf;
use std::time::{Duration, Instant};
use viewport::{display_downscale_scale, ViewportState, DEFAULT_MAX_TEXTURE_DIM};

pub(crate) const SUCCESS_MESSAGE_DURATION: Duration = Duration::from_secs(4);
/// Pixel step for a single wheel "line" of scrolling.
pub(crate) const WHEEL_LINE_PX: f32 = 60.0;

use annotation_defaults::AnnotationDefaults;

// ---------------------------------------------------------------------------
// Annotation defaults state
// ---------------------------------------------------------------------------

pub(crate) struct AnnotationDefaultsState {
    pub values: AnnotationDefaults,
    pub config_path: Option<PathBuf>,
    pub warning_reported: bool,
}

use iced::{Point, Size, Vector};

// ---------------------------------------------------------------------------
// Inline message
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InlineMessage {
    Success { text: String, expires_at: Instant },
    Error(String),
    Warning(String),
}

impl InlineMessage {
    pub fn text(&self) -> &str {
        match self {
            InlineMessage::Success { text, .. } => text,
            InlineMessage::Error(text) => text,
            InlineMessage::Warning(text) => text,
        }
    }

    pub(crate) fn success(text: String) -> Self {
        InlineMessage::Success {
            text,
            expires_at: Instant::now() + SUCCESS_MESSAGE_DURATION,
        }
    }

    pub(crate) fn is_error(&self) -> bool {
        matches!(self, InlineMessage::Error(_))
    }

    pub(crate) fn expiry(&self) -> Option<Instant> {
        match self {
            InlineMessage::Success { expires_at, .. } => Some(*expires_at),
            InlineMessage::Error(_) | InlineMessage::Warning(_) => None,
        }
    }
}

// ---------------------------------------------------------------------------
// Issue Pack dialog
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum IssuePackKind {
    Folder,
    Zip,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct IssuePackDialog {
    pub review_confirmed: bool,
    pub pending_kind: Option<IssuePackKind>,
}

impl IssuePackDialog {
    pub(crate) fn new() -> Self {
        Self {
            review_confirmed: false,
            pending_kind: None,
        }
    }
}

// ---------------------------------------------------------------------------
// Workspace
// ---------------------------------------------------------------------------

pub struct ResultWorkspace {
    pub document: ResultDocument,
    pub message: Option<InlineMessage>,
    pub pending_discard: Option<DiscardPrompt>,
    pub pending_unredacted_action: Option<secure_sharing::UnredactedAction>,
    /// Iced image handle built once. For oversized captures this is a
    /// downscaled display copy (spec §9.6); the document keeps the full source.
    pub image_handle: ImageHandle,
    /// Current zoom mode + scroll offset state.
    pub viewport: ViewportState,
    /// Current keyboard modifiers, tracked for wheel routing.
    pub modifiers: iced::keyboard::Modifiers,
    /// Last known canvas pointer position (scrollable-local, for
    /// pointer-anchored zoom). Fed solely by the canvas `mouse_area.on_move`.
    pub pointer_position: Point,
    /// Last reported scrollable bounds (visible canvas area).
    pub viewport_bounds: Size,
    /// Identity of the single canvas scrollable, for scroll operations.
    pub scrollable_id: iced::widget::Id,
    /// UI/session editor state (active tool, selection, drafts, Navigator).
    pub editor: canvas::EditorState,
    /// Workspace mode: Normal or Workbench (Smart Redaction).
    pub mode: workbench::WorkspaceMode,
    /// Open Issue Pack export dialog, if any.
    pub issue_pack: Option<IssuePackDialog>,
    /// OCR text state (selectable text overlay).
    #[cfg(feature = "ocr")]
    pub ocr_text: ocr_text::OcrTextState,
    /// Identity of the inline text editor widget, for focus operations.
    #[allow(dead_code)]
    pub text_editor_id: iced::widget::Id,
    /// Loaded annotation style defaults from the shared config.toml.
    pub(crate) annotation_defaults: AnnotationDefaultsState,
}

impl ResultWorkspace {
    pub fn new(document: ResultDocument, initial_error: Option<String>) -> Self {
        let (annotation_defaults, warning) =
            match crate::daemon::config::config_path().ok().and_then(|p| {
                let loaded = annotation_defaults::load_from(&p);
                Some((loaded, p))
            }) {
                Some((loaded, path)) => (
                    AnnotationDefaultsState {
                        values: loaded.values,
                        config_path: Some(path),
                        warning_reported: !loaded.warnings.is_empty(),
                    },
                    loaded.warnings.into_iter().next(),
                ),
                None => (
                    AnnotationDefaultsState {
                        values: AnnotationDefaults::default(),
                        config_path: None,
                        warning_reported: false,
                    },
                    None,
                ),
            };
        let mut ws = Self::with_max_texture_dim(document, initial_error, DEFAULT_MAX_TEXTURE_DIM);
        ws.annotation_defaults = annotation_defaults;
        if let Some(warn) = warning {
            ws.message = Some(InlineMessage::Warning(warn));
        }
        ws
    }

    /// Construct with an explicit texture ceiling (used by tests; production
    /// uses [`DEFAULT_MAX_TEXTURE_DIM`]).
    pub fn with_max_texture_dim(
        document: ResultDocument,
        initial_error: Option<String>,
        max_texture_dim: u32,
    ) -> Self {
        let source_size = Size::new(
            document.image.source().width() as f32,
            document.image.source().height() as f32,
        );
        let scale = display_downscale_scale(source_size, max_texture_dim);
        let image_handle = build_display_handle(document.image.source(), scale);

        let message = if let Some(err) = initial_error {
            Some(InlineMessage::Error(err))
        } else {
            document
                .source_path
                .as_deref()
                .map(|path| InlineMessage::success(format!("Saved to {}", path.display())))
        };

        let zoom = viewport::default_zoom(source_size);

        Self {
            pending_discard: None,
            pending_unredacted_action: None,
            editor: canvas::EditorState::new(
                document.image.state_id(),
                viewport::is_tall_image(source_size),
            ),
            mode: workbench::WorkspaceMode::Normal,
            issue_pack: None,
            #[cfg(feature = "ocr")]
            ocr_text: ocr_text::OcrTextState::idle(),
            text_editor_id: iced::widget::Id::unique(),
            annotation_defaults: AnnotationDefaultsState {
                values: AnnotationDefaults::default(),
                config_path: None,
                warning_reported: false,
            },
            document,
            message,
            image_handle,
            viewport: ViewportState {
                zoom,
                scroll_offset: Vector::new(0.0, 0.0),
            },
            modifiers: iced::keyboard::Modifiers::default(),
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

    /// Seed the viewport bounds with the expected canvas size (window minus chrome).
    ///
    /// Prevents the initial fit-mode render from computing a degenerate scale
    /// before the scrollable has reported its real bounds. A pixel or two of
    /// inaccuracy is harmless — the scrollable corrects it on the next frame.
    pub fn with_initial_viewport(mut self, bounds: Size) -> Self {
        self.viewport_bounds = bounds;
        self
    }

    /// Reveal is only meaningful once the capture has a durable path on disk.
    #[allow(dead_code)]
    pub fn can_reveal(&self) -> bool {
        self.document.reveal_path().is_some()
    }

    pub fn annotations_dirty(&self) -> bool {
        self.document.image.state_id() != self.editor.saved_state_id
    }

    pub(crate) fn has_secure_redactions(&self) -> bool {
        secure_sharing::has_secure_redactions(&self.document)
    }

    /// Original (full-resolution) image dimensions, reported by the status bar
    /// regardless of any display downscale.
    pub(crate) fn original_size(&self) -> Size {
        let (w, h) = self.document.image.source().dimensions();
        Size::new(w as f32, h as f32)
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
    pub fn apply_save_as(
        &mut self,
        result: Result<Option<PathBuf>, String>,
        saved_state_id: u64,
        safe_output: bool,
    ) {
        match result {
            Ok(Some(path)) => {
                let text = if safe_output {
                    secure_sharing::SAVE_SAFE_SUCCESS.to_string()
                } else {
                    format!("Saved to {}", path.display())
                };
                self.document.last_export_path = Some(path);
                self.document.last_export_is_safe = safe_output;
                self.editor.saved_state_id = saved_state_id;
                self.message = Some(InlineMessage::success(text));
                self.pending_discard = None;
            }
            Ok(None) => {}
            Err(e) => self.message = Some(InlineMessage::Error(e)),
        }
    }
}

/// Build an iced image handle from the source, downscaling when `scale < 1.0`
/// (spec §9.6). `scale == 1.0` uses the full-resolution pixels directly.
pub(crate) fn build_display_handle(source: &RgbaImage, scale: f32) -> ImageHandle {
    if scale >= 1.0 {
        return ImageHandle::from_rgba(source.width(), source.height(), source.as_raw().clone());
    }
    let w = ((source.width() as f32 * scale).round() as u32).max(1);
    let h = ((source.height() as f32 * scale).round() as u32).max(1);
    let resized = image::imageops::resize(source, w, h, image::imageops::FilterType::Triangle);
    ImageHandle::from_rgba(w, h, resized.into_raw())
}

// ---------------------------------------------------------------------------
// Runner (Linux standalone window)
// ---------------------------------------------------------------------------

#[cfg(target_os = "linux")]
pub fn run(document: ResultDocument, initial_error: Option<String>) -> Result<(), String> {
    use std::sync::{Arc, Mutex};

    let boot_data = Arc::new(Mutex::new(Some((document, initial_error))));

    // Estimated canvas area: window (1100×760) minus padding (16×16),
    // toolbar (~35), status bar (~35), and column spacing (3 × 8).
    // A pixel or two of inaccuracy is fine — the scrollable corrects it.
    const INITIAL_VIEWPORT: Size = Size::new(1084.0, 650.0);

    let boot = move || {
        let (document, initial_error) = boot_data
            .lock()
            .unwrap()
            .take()
            .expect("result workspace boot data already consumed");
        (
            ResultWorkspace::new(document, initial_error).with_initial_viewport(INITIAL_VIEWPORT),
            iced::Task::none(),
        )
    };

    iced::application(boot, update, view)
        .title("Rollshot")
        .font(rollshot_image_document::style::FONT_REGULAR_BYTES)
        .font(rollshot_image_document::style::FONT_BOLD_BYTES)
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
    use super::viewport::ZoomMode;
    use super::*;
    use iced::Size as IcedSize;
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
    fn save_as_success_updates_export_path_and_message() {
        let mut state = workspace();
        let saved_state_id = state.document.image.state_id();
        state.apply_save_as(
            Ok(Some(PathBuf::from("/tmp/result.png"))),
            saved_state_id,
            false,
        );
        assert_eq!(
            state.document.last_export_path.as_deref(),
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
        let saved_state_id = state.document.image.state_id();
        state.apply_save_as(Ok(None), saved_state_id, false);
        assert!(state.document.last_export_path.is_none());
        assert!(state.message.is_none());
    }

    #[test]
    fn save_as_error_sets_persistent_error_and_no_path() {
        let mut state = workspace();
        let saved_state_id = state.document.image.state_id();
        state.apply_save_as(Err("write failed".to_string()), saved_state_id, false);
        assert!(state.document.last_export_path.is_none());
        assert!(matches!(&state.message, Some(InlineMessage::Error(e)) if e == "write failed"));
    }

    // -- resize behavior -----------------------------------------------------

    #[test]
    fn resize_keeps_custom_zoom_percentage() {
        let mut state = workspace();
        state.viewport.zoom = ZoomMode::Custom(150);
        state.apply_viewport_bounds(IcedSize::new(900.0, 700.0));
        assert_eq!(state.viewport.zoom, ZoomMode::Custom(150));
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
        let saved_state_id = state.document.image.state_id();
        state.apply_save_as(
            Ok(Some(PathBuf::from("/tmp/result.png"))),
            saved_state_id,
            false,
        );
        assert!(state.can_reveal());
    }

    // -- §9.6 display downscale ----------------------------------------------

    #[test]
    fn small_image_uses_full_resolution_and_reports_original_dims() {
        let img = RgbaImage::from_pixel(640, 480, Rgba([1, 2, 3, 255]));
        let state = ResultWorkspace::with_max_texture_dim(ResultDocument::unsaved(img), None, 8192);
        // Original dimensions reported regardless of display copy.
        assert_eq!(state.original_size(), IcedSize::new(640.0, 480.0));
    }

    #[test]
    fn oversized_image_keeps_full_res_source_and_original_dims() {
        // Long capture: height exceeds a small ceiling so the display copy is
        // downscaled, but the source + reported dims stay original.
        let img = RgbaImage::from_pixel(100, 400, Rgba([9, 9, 9, 255]));
        let state = ResultWorkspace::with_max_texture_dim(ResultDocument::unsaved(img), None, 200);
        assert_eq!(state.document.image.source().dimensions(), (100, 400));
        assert_eq!(state.original_size(), IcedSize::new(100.0, 400.0));
    }

    /// Both axes over the ceiling: the DISPLAY handle is downscaled so both
    /// display dims land at/under the ceiling, while the source + reported
    /// original dims stay at the full resolution.
    #[test]
    fn oversized_both_axes_downscales_display_handle_only() {
        let ceiling = 128u32;
        let img = RgbaImage::from_pixel(250, 160, Rgba([7, 7, 7, 255]));
        let state =
            ResultWorkspace::with_max_texture_dim(ResultDocument::unsaved(img), None, ceiling);

        // Source + status-bar dims stay full resolution.
        assert_eq!(state.document.image.source().dimensions(), (250, 160));
        assert_eq!(state.original_size(), IcedSize::new(250.0, 160.0));

        // Display handle was downscaled: both axes at/under the ceiling.
        let (dw, dh) = match state.image_handle.clone() {
            ImageHandle::Rgba { width, height, .. } => (width, height),
            _ => panic!("expected an rgba display handle"),
        };
        assert!(dw <= ceiling, "display width {dw} should be <= {ceiling}");
        assert!(dh <= ceiling, "display height {dh} should be <= {ceiling}");
        // The longest axis (250) drives the scale, so the display copy is
        // strictly smaller than the source on that axis.
        assert!(dw < 250, "display width {dw} should be downscaled");
    }

    // -- discard modal / save interaction ------------------------------------

    #[test]
    fn save_as_success_clears_pending_discard_prompt() {
        let mut state = unsaved_workspace();
        let _ = update(&mut state, Message::RequestClose);
        assert!(
            state.pending_discard.is_some(),
            "unsaved close should prompt"
        );
        let saved_state_id = state.document.image.state_id();
        state.apply_save_as(
            Ok(Some(PathBuf::from("/tmp/result.png"))),
            saved_state_id,
            false,
        );
        assert!(
            state.pending_discard.is_none(),
            "a successful save should close the discard prompt"
        );
    }
}
