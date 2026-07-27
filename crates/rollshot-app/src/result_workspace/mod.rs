pub mod actions;
pub(crate) mod annotation_defaults;
pub(crate) mod box_tool;
pub(crate) mod canvas;
mod document;
mod freehand_tool;
mod navigator;
#[cfg(feature = "ocr")]
pub(crate) mod ocr_layer;
#[cfg(feature = "ocr")]
pub(crate) mod ocr_text;
#[allow(dead_code)]
pub(crate) mod pixelate_preview;
pub(crate) mod properties;
mod secure_sharing;
pub(crate) mod toolbar;
pub(crate) mod two_point;
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
use sha2::{Digest, Sha256};
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
use pixelate_preview::PixelatePreviewCache;

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
    /// Cached SHA-256 digest of the source image bytes (immutable for the session).
    pub base_image_digest: [u8; 32],
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
    /// Pixelate preview cache: keys → RGBA handles, LRU eviction.
    pub(crate) pixelate_previews: PixelatePreviewCache,
}

impl ResultWorkspace {
    pub fn new(document: ResultDocument, initial_error: Option<String>) -> Self {
        Self::with_config_path(
            document,
            initial_error,
            crate::daemon::config::config_path().ok(),
        )
    }

    /// Construct with an explicit annotation-defaults config path. `None`
    /// skips loading and uses canonical defaults. Tests use this to stay
    /// isolated from the user's real config.toml; production uses [`Self::new`].
    pub(crate) fn with_config_path(
        document: ResultDocument,
        initial_error: Option<String>,
        config_path: Option<PathBuf>,
    ) -> Self {
        let (annotation_defaults, warning) = match config_path.map(|p| {
            let loaded = annotation_defaults::load_from(&p);
            (loaded, p)
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
        } else if let document::DocumentOrigin::SavedCapture(ref path) = document.origin {
            Some(InlineMessage::success(format!(
                "Saved to {}",
                path.display()
            )))
        } else {
            None
        };

        let zoom = viewport::default_zoom(source_size);

        let base_image_digest: [u8; 32] = {
            let hash = Sha256::digest(document.image.source().as_raw());
            let mut out = [0u8; 32];
            out.copy_from_slice(&hash);
            out
        };

        Self {
            pending_discard: None,
            pending_unredacted_action: None,
            base_image_digest,
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
            pixelate_previews: PixelatePreviewCache::new(64 * 1024 * 1024),
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

    pub(crate) fn document_status_text(&self) -> Option<&'static str> {
        self.document.origin_status(self.annotations_dirty())
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
    use iced_test::Simulator;
    use image::Rgba;
    use std::path::Path;

    fn image() -> RgbaImage {
        RgbaImage::from_pixel(2, 2, Rgba([100, 150, 200, 255]))
    }

    fn workspace() -> ResultWorkspace {
        ResultWorkspace::with_config_path(ResultDocument::unsaved(image()), None, None)
    }

    fn unsaved_workspace() -> ResultWorkspace {
        ResultWorkspace::with_config_path(ResultDocument::unsaved(image()), None, None)
    }

    fn saved_workspace() -> ResultWorkspace {
        ResultWorkspace::with_config_path(
            ResultDocument::saved(image(), PathBuf::from("/tmp/result.png")),
            None,
            None,
        )
    }

    fn simulator_at(state: &ResultWorkspace, size: IcedSize) -> Simulator<'_, Message> {
        Simulator::with_size(
            iced::Settings {
                fonts: vec![
                    rollshot_image_document::style::FONT_REGULAR_BYTES.into(),
                    rollshot_image_document::style::FONT_BOLD_BYTES.into(),
                ],
                ..iced::Settings::default()
            },
            size,
            view(state),
        )
    }

    #[test]
    fn result_workspace_chrome_is_visible_at_supported_window_sizes() {
        for size in [IcedSize::new(1100.0, 760.0), IcedSize::new(640.0, 420.0)] {
            let state = workspace().with_initial_viewport(size);
            let scrollable_id = state.scrollable_id.clone();
            let mut ui = simulator_at(&state, size);

            let canvas = ui.find(scrollable_id).expect("canvas scrollable exists");
            assert!(
                (80.0..120.0).contains(&canvas.bounds().y),
                "canvas is not directly below the two-row toolbar at {size:?}: canvas={:?}",
                canvas.bounds()
            );

            for label in [
                "Close",
                "Copy",
                "Save As",
                "Fit Width",
                "Fit Window",
                "100%",
            ] {
                let target = ui
                    .find(label)
                    .unwrap_or_else(|error| panic!("{label:?} missing at {size:?}: {error}"));
                let bounds = target.bounds();
                let visible = target
                    .visible_bounds()
                    .unwrap_or_else(|| panic!("{label:?} is not visible at {size:?}"));
                assert!(
                    (visible.width - bounds.width).abs() < 0.01
                        && (visible.height - bounds.height).abs() < 0.01,
                    "{label:?} is clipped at {size:?}: bounds={bounds:?}, visible={visible:?}"
                );
            }
        }
    }

    #[test]
    fn result_workspace_overflow_menu_expands_toolbar_without_covering_canvas() {
        let size = IcedSize::new(1100.0, 760.0);
        let mut state = workspace().with_initial_viewport(IcedSize::new(1084.0, 650.0));
        state.editor.more_menu_open = true;
        let scrollable_id = state.scrollable_id.clone();
        let mut ui = simulator_at(&state, size);

        let menu_item = ui
            .find("Smart Redaction")
            .expect("overflow menu is rendered");
        assert!(
            menu_item.visible_bounds().is_some(),
            "overflow menu is clipped"
        );

        let canvas = ui.find(scrollable_id).expect("canvas scrollable exists");
        assert!(
            (115.0..170.0).contains(&canvas.bounds().y),
            "expanded toolbar does not reserve one menu row: canvas={:?}",
            canvas.bounds()
        );
    }

    #[test]
    fn result_workspace_color_picker_expands_toolbar_without_covering_canvas() {
        let size = IcedSize::new(1100.0, 760.0);
        let mut state = workspace().with_initial_viewport(IcedSize::new(1084.0, 650.0));
        let color = rollshot_image_document::Rgb8::new(0xE5, 0x48, 0x4D);
        state.editor.properties.color = Some(super::properties::ColorTransaction {
            target: super::properties::PropertyTarget::NumberTool,
            property: super::properties::ColorProperty::NumberAccent,
            original: color,
            preview: color,
            hex: "#E5484D".to_owned(),
        });
        let scrollable_id = state.scrollable_id.clone();
        let mut ui = simulator_at(&state, size);

        let apply = ui.find("Apply").expect("color picker is rendered");
        assert!(apply.visible_bounds().is_some(), "color picker is clipped");

        let canvas = ui.find(scrollable_id).expect("canvas scrollable exists");
        assert!(
            (250.0..380.0).contains(&canvas.bounds().y),
            "expanded toolbar does not reserve color picker height: canvas={:?}",
            canvas.bounds()
        );
    }

    #[test]
    #[ignore = "writes visual debugging artifacts"]
    fn render_result_workspace_visual_scenarios() {
        let artifact_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/ui-artifacts/result-workspace");

        for (name, size, viewport) in [
            (
                "standard-1100x760",
                IcedSize::new(1100.0, 760.0),
                IcedSize::new(1084.0, 650.0),
            ),
            (
                "minimum-640x420",
                IcedSize::new(640.0, 420.0),
                IcedSize::new(624.0, 310.0),
            ),
        ] {
            let state = workspace().with_initial_viewport(viewport);
            let mut ui = simulator_at(&state, size);
            let snapshot = ui.snapshot(&iced::Theme::Dark).expect("render scenario");
            let base = artifact_dir.join(name);

            for renderer in ["tiny-skia", "wgpu"] {
                let _ = std::fs::remove_file(base.with_file_name(format!("{name}-{renderer}.png")));
            }

            assert!(snapshot.matches_image(base).expect("write scenario PNG"));
        }
    }

    // -- config isolation (with_config_path) ---------------------------------

    #[test]
    fn no_config_path_uses_canonical_defaults_without_message() {
        let state = workspace();
        assert!(state.message.is_none());
        assert_eq!(
            state.annotation_defaults.values,
            annotation_defaults::AnnotationDefaults::default()
        );
        assert!(state.annotation_defaults.config_path.is_none());
    }

    #[test]
    fn malformed_config_surfaces_warning_message() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(&path, "not valid toml [[[").unwrap();
        let state = ResultWorkspace::with_config_path(
            ResultDocument::unsaved(image()),
            None,
            Some(path.clone()),
        );
        assert!(matches!(&state.message, Some(InlineMessage::Warning(_))));
        assert!(state.annotation_defaults.warning_reported);
        assert_eq!(state.annotation_defaults.config_path, Some(path));
    }

    #[test]
    fn valid_config_values_load_through_with_config_path() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(&path, "[annotation_defaults.line]\nwidth = 7.0\n").unwrap();
        let state =
            ResultWorkspace::with_config_path(ResultDocument::unsaved(image()), None, Some(path));
        assert!(state.message.is_none());
        assert_eq!(state.annotation_defaults.values.line.width, 7.0);
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
        let state = ResultWorkspace::with_config_path(
            ResultDocument::saved(image(), path.clone()),
            None,
            None,
        );
        assert_eq!(
            state.message_text(),
            Some(format!("Saved to {}", path.display()))
        );
    }

    #[test]
    fn unsaved_workspace_with_initial_error_has_error_message() {
        let err = "disk full".to_string();
        let state = ResultWorkspace::with_config_path(
            ResultDocument::unsaved(image()),
            Some(err.clone()),
            None,
        );
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

    // -- imported workspace status (Task 5) ----------------------------------

    fn imported_workspace() -> (tempfile::TempDir, ResultWorkspace) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("source.png");
        image()
            .save_with_format(&path, image::ImageFormat::Png)
            .unwrap();
        let imported = crate::image_import::load(&path).unwrap();
        let state = ResultWorkspace::with_config_path(
            ResultDocument::imported(imported.pixels, imported.source),
            None,
            None,
        );
        (dir, state)
    }

    #[test]
    fn imported_workspace_status_is_visible_and_tracks_dirty_state() {
        let (_dir, mut state) = imported_workspace();
        assert_eq!(state.document_status_text(), Some("Imported"));

        {
            let mut ui = simulator_at(&state, IcedSize::new(1100.0, 760.0));
            assert!(ui.find("Imported").is_ok());
        }

        state
            .document
            .image
            .add_text_note(
                rollshot_image_document::ImagePoint::new(1.0, 1.0),
                "note".to_string(),
            )
            .unwrap();
        assert_eq!(
            state.document_status_text(),
            Some("Imported • Unsaved edits")
        );
        let mut ui = simulator_at(&state, IcedSize::new(1100.0, 760.0));
        assert!(ui.find("Imported • Unsaved edits").is_ok());
    }

    #[test]
    fn imported_workspace_has_no_saved_to_message() {
        let (_dir, state) = imported_workspace();
        // Imported documents must not show "Saved to ..." — nothing was saved.
        assert!(state.message.is_none(), "message = {:?}", state.message);
    }

    #[test]
    #[ignore = "writes visual debugging artifacts"]
    fn render_imported_workspace_visual_scenarios() {
        let artifact_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/ui-artifacts/result-workspace");

        for (label, size, dirty) in [
            (
                "imported-clean-1100x760",
                IcedSize::new(1100.0, 760.0),
                false,
            ),
            ("imported-clean-640x420", IcedSize::new(640.0, 420.0), false),
            (
                "imported-dirty-1100x760",
                IcedSize::new(1100.0, 760.0),
                true,
            ),
            ("imported-dirty-640x420", IcedSize::new(640.0, 420.0), true),
        ] {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("source.png");
            image()
                .save_with_format(&path, image::ImageFormat::Png)
                .unwrap();
            let imported = crate::image_import::load(&path).unwrap();
            let mut state = ResultWorkspace::with_config_path(
                ResultDocument::imported(imported.pixels, imported.source),
                None,
                None,
            );
            if dirty {
                state
                    .document
                    .image
                    .add_text_note(
                        rollshot_image_document::ImagePoint::new(1.0, 1.0),
                        "note".to_string(),
                    )
                    .unwrap();
            }
            let mut ui = simulator_at(&state, size);
            let snapshot = ui.snapshot(&iced::Theme::Dark).expect(label);
            let base = artifact_dir.join(label);
            assert!(snapshot.matches_image(base).expect("write scenario PNG"));
        }
    }

    // -- Task 7: UI evidence for restored agent review state ------------------

    fn workbench_proposal_with_candidate() -> rollshot_edit_proposal::EditProposal {
        use rollshot_edit_proposal::{
            CandidateId, ConfidenceSummary, ProposalId, ProposedCandidate, ProposedEdit,
            Provenance, ProvenanceSource,
        };
        rollshot_edit_proposal::EditProposal {
            id: ProposalId::parse("proposal-00000001-0000-4000-8000-000000000000").unwrap(),
            base_document_state_id: 0,
            candidates: vec![ProposedCandidate {
                id: CandidateId(1),
                edit: ProposedEdit::AddRedaction {
                    bounds: rollshot_image_document::ImageRect {
                        x: 0.0,
                        y: 0.0,
                        width: 10.0,
                        height: 10.0,
                    },
                },
                confidence: 0.9,
                label: "pending redaction".into(),
                rationale: None,
                provenance: Provenance {
                    source: ProvenanceSource::Manual,
                },
            }],
            confidence_summary: ConfidenceSummary::from_confidences(&[0.9]),
            rationale_summary: None,
            provenance: Provenance {
                source: ProvenanceSource::Manual,
            },
        }
    }

    fn workspace_with_restored_review(size: IcedSize) -> ResultWorkspace {
        let mut ws = workspace();
        ws.mode = workbench::WorkspaceMode::Workbench(workbench::WorkbenchState::default());
        {
            let wb = match &mut ws.mode {
                workbench::WorkspaceMode::Workbench(wb) => wb,
                _ => unreachable!(),
            };
            let proposal = workbench_proposal_with_candidate();
            let ids: Vec<_> = proposal.candidates.iter().map(|c| c.id).collect();
            wb.pending_proposal = Some(proposal);
            wb.review = workbench::CandidateReview::from_candidates(&ids);
            wb.cached_base_digest = Some(ws.base_image_digest);
        }
        ws.with_initial_viewport(size)
    }

    #[test]
    fn restored_review_matches_existing_review_structure() {
        let size = IcedSize::new(1100.0, 760.0);
        let state = workspace_with_restored_review(size);
        let mut ui = simulator_at(&state, size);

        // Restored review with 1 pending candidate shows the Apply button.
        let apply = ui
            .find("Apply 1 redactions")
            .expect("restored review must show Apply button");
        let bounds = apply.bounds();
        let visible = apply
            .visible_bounds()
            .expect("Apply button must be visible");
        assert!(
            (visible.width - bounds.width).abs() < 0.1
                && (visible.height - bounds.height).abs() < 0.1,
            "Apply button must not be clipped: bounds={bounds:?}, visible={visible:?}"
        );

        // Clicking Apply emits the ApplyCandidates message.
        let _ = ui.click("Apply 1 redactions").expect("click apply");
        let msgs: Vec<_> = ui.into_messages().collect();
        assert!(
            msgs.iter().any(|m| matches!(
                m,
                Message::Workbench(workbench::WorkbenchMessage::ApplyCandidates)
            )),
            "click must emit ApplyCandidates message"
        );
    }

    #[test]
    fn stale_restored_review_has_no_apply_action() {
        use rollshot_agent::product_task::SourceBinding;

        let size = IcedSize::new(1100.0, 760.0);
        let mut ws = workspace();
        ws.mode = workbench::WorkspaceMode::Workbench(workbench::WorkbenchState::default());

        // Simulate a stale restore: digest mismatch causes restore to be
        // silently dropped, leaving the workbench empty (no proposal).
        let op_id = {
            let wb = match &mut ws.mode {
                workbench::WorkspaceMode::Workbench(wb) => wb,
                _ => unreachable!(),
            };
            wb.cached_base_digest = Some([99u8; 32]); // mismatch with source_binding
            wb.restore_operation_id.next()
        };
        let binding = SourceBinding::new([1u8; 32], [2u8; 32], 0, "preset-default".into(), None);
        let _ = update(
            &mut ws,
            Message::Workbench(workbench::WorkbenchMessage::TaskRestoreFinished {
                operation_id: op_id,
                source_binding: binding,
                result: Some(make_ready_snapshot()),
            }),
        );

        // Stale restore leaves workbench empty — no Apply button.
        let wb = match &ws.mode {
            workbench::WorkspaceMode::Workbench(wb) => wb,
            _ => unreachable!(),
        };
        assert!(
            wb.pending_proposal.is_none(),
            "stale restore must not populate proposal"
        );

        let mut ui = simulator_at(&ws, size);
        assert!(
            ui.find("Apply 1 redactions").is_err(),
            "stale restored review must not show Apply button"
        );
    }

    fn make_ready_snapshot() -> rollshot_agent::product_task::ProductTaskSnapshot {
        use rollshot_agent::product_task::{
            ArtifactId, ArtifactKind, ArtifactRevision, PayloadSourceV1, ProductArtifactMetadata,
            ProductTaskId, SmartRedactionReviewPayload, SourceBinding, TaskAttempt, TaskAttemptId,
            TaskKind,
        };

        let task_id = ProductTaskId::parse("task-00000000-0000-4000-8000-000000000001").unwrap();
        let run_id =
            rollshot_agent::domain::RunId::parse("run-00000000-0000-4000-8000-000000000001")
                .unwrap();
        let binding = SourceBinding::new([1u8; 32], [2u8; 32], 0, "preset-default".into(), None);
        let snapshot = rollshot_agent::product_task::ProductTaskSnapshot::new(
            task_id.clone(),
            TaskKind::SmartRedactionAuthor,
            binding.clone(),
            10,
        )
        .unwrap();
        let attempt = TaskAttempt::new(TaskAttemptId::new(1), run_id.clone(), 10);
        let running = snapshot.start_attempt(attempt, 20).unwrap();
        let metadata = ProductArtifactMetadata::new(
            ArtifactId::parse("artifact-00000000-0000-4000-8000-000000000001").unwrap(),
            ArtifactRevision::new(1),
            ArtifactKind::SmartRedaction,
            1,
            String::new(),
            binding,
            task_id.clone(),
            running.attempts().last().unwrap().attempt_id(),
            run_id,
            "proposal-00000001-0000-4000-8000-000000000000".to_owned(),
            "anthropic".into(),
            "claude".into(),
            String::new(),
            1,
            0.42,
            30,
        );
        let payload = SmartRedactionReviewPayload {
            source: PayloadSourceV1 {
                kind: "agent_run".into(),
                validation_summary: "5 nodes".into(),
            },
            proposal: rollshot_agent::product_task::PayloadProposalV1 {
                proposal_id: "proposal-00000001-0000-4000-8000-000000000000".into(),
                candidate_count: 1,
            },
            dry_run: rollshot_agent::product_task::PayloadDryRunV1 {
                candidate_count: 1,
                affected_area: 0.42,
            },
            config: rollshot_agent::product_task::PayloadConfigV1 {
                provider: "anthropic".into(),
                model: "claude".into(),
                payload_mode: rollshot_agent::product_task::PayloadMode::Author,
                run_kind: "smart_redaction".into(),
                budget_dimensions: std::collections::BTreeMap::new(),
            },
        };
        let proposal_bytes = serde_json::to_vec(&workbench_proposal_with_candidate()).unwrap();
        running
            .record_ready_for_review(
                metadata,
                payload,
                Some(proposal_bytes),
                30,
            )
            .unwrap()
    }

    #[test]
    #[ignore = "writes visual evidence artifacts"]
    fn render_product_task_restore_visual_evidence() {
        let artifact_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/ui-artifacts/product-task-artifact");
        // Clean stale artifacts so matches_image creates fresh baselines.
        let _ = std::fs::remove_dir_all(&artifact_dir);

        for size in [IcedSize::new(1100.0, 760.0), IcedSize::new(640.0, 420.0)] {
            let tag = format!("{}x{}", size.width as u32, size.height as u32);

            // Expected: workbench with pending proposal and review.
            let expected_state = workspace_with_restored_review(size);
            let mut expected_ui = simulator_at(&expected_state, size);
            let expected_snap = expected_ui
                .snapshot(&iced::Theme::Dark)
                .expect("render expected");
            let expected_path = artifact_dir.join(format!("expected-{tag}"));
            assert!(
                expected_snap
                    .matches_image(&expected_path)
                    .expect("write expected PNG"),
                "expected image written"
            );

            // Restored: identical state after successful restore.
            let restored_state = workspace_with_restored_review(size);
            let mut restored_ui = simulator_at(&restored_state, size);
            let restored_snap = restored_ui
                .snapshot(&iced::Theme::Dark)
                .expect("render restored");
            let restored_path = artifact_dir.join(format!("restored-{tag}"));
            assert!(
                restored_snap
                    .matches_image(&restored_path)
                    .expect("write restored PNG"),
                "restored image matches expected"
            );

            // Diff: should be identical (AE=0) for successful restore.
            let diff_path = artifact_dir.join(format!("diff-{tag}"));
            assert!(
                expected_snap
                    .matches_image(&diff_path)
                    .expect("write diff PNG"),
                "diff image matches expected (AE=0)"
            );
        }
    }
}
