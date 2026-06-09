//! Single-process macOS product daemon (spec §4.4).
//!
//! One `iced::daemon` owned by `rollshot-app` drives the whole post-capture
//! flow as windows/phases inside ONE event loop — there is never a second event
//! loop. The phases are:
//!
//! - [`Phase::Capture`] — embeds [`rollshot_iced_overlay::macos_capture::Component`]
//!   (Task 7), hosting its overlay + controls windows and mapping its
//!   [`HostEffect`]s.
//! - [`Phase::Thumbnail`] — the floating saved-capture thumbnail
//!   ([`crate::macos_thumbnail`]) with an 8s auto-dismiss timer.
//! - [`Phase::Workspace`] — the reusable Result Workspace
//!   ([`crate::result_workspace`]), reached on auto-save failure (unsaved) or
//!   when the user clicks the thumbnail (saved, reusing the in-memory document).
//!
//! Capture completion transitions in-loop: `complete_capture` keeps the
//! `RgbaImage` in memory, calls [`crate::storage::auto_save`], and applies
//! [`crate::post_capture::select_presentation`] for [`Platform::Macos`] to pick
//! the thumbnail vs. unsaved-workspace presentation.
//!
//! NOTE: this module is `#[cfg(target_os = "macos")]` and is not compiled on
//! Linux (it embeds the macOS-only capture component). Its compilation and the
//! product-phase unit tests below are verified on a macOS machine; the portable
//! thumbnail timer/interaction logic in [`crate::macos_thumbnail`] is tested on
//! all hosts.

use std::time::Instant;

use iced::{window, Element, Point, Size, Task};
use image::RgbaImage;

use rollshot_capture::CaptureMode;
use rollshot_iced_overlay::macos_capture::{Component, HostEffect};
use rollshot_iced_overlay::{CaptureResult, OverlayConfig};

use crate::macos_thumbnail::{self, release_action, ThumbnailAction, ThumbnailState};
use crate::post_capture::{select_presentation, Presentation};
use crate::result_workspace::{self, ResultDocument, ResultWorkspace};
use crate::storage::{self, Platform};

/// Messages handled by the product daemon. Capture/workspace variants forward to
/// their owners; the remaining variants drive the thumbnail phase and host-side
/// window-open resolutions.
#[derive(Debug, Clone)]
pub enum Message {
    /// A capture-component message (opaque to the host except for window-ready).
    Capture(rollshot_iced_overlay::macos_capture::Message),
    /// A Result Workspace message.
    Workspace(result_workspace::Message),
    /// Thumbnail pressed (button down).
    ThumbnailPressed,
    /// Thumbnail released (button up): click vs. native-drag decision.
    ThumbnailReleased,
    /// Pointer entered/left the thumbnail card.
    ThumbnailHoverChanged(bool),
    /// Latest cursor position over the thumbnail window, for press/drag math.
    ThumbnailCursorMoved(Point),
    /// Periodic tick driving the thumbnail auto-dismiss timer.
    ThumbnailTick(Instant),
    /// The thumbnail window finished opening.
    ThumbnailWindowReady(window::Id),
    /// The workspace window finished opening.
    WorkspaceWindowReady(window::Id),
}

/// The current phase of the product daemon.
pub enum Phase {
    Capture(Component),
    Thumbnail(ThumbnailState),
    Workspace(ResultWorkspace),
}

/// The single daemon state.
pub struct MacosProduct {
    phase: Phase,
    /// The in-memory capture document, kept across the thumbnail→workspace
    /// transition so a thumbnail click never reloads the image from disk.
    document: Option<ResultDocument>,
    thumbnail_window: Option<window::Id>,
    workspace_window: Option<window::Id>,
    /// Latest known cursor position over the thumbnail window.
    thumbnail_cursor: Point,
}

impl MacosProduct {
    /// Build the daemon state, embedding the capture component and producing the
    /// task that opens its overlay window. Returns `Ok(None)` when the user
    /// cancelled before any capture began (the caller then skips the daemon
    /// entirely), or `Err` if capture setup failed.
    pub fn new(config: OverlayConfig) -> Result<Option<(Self, Task<Message>)>, String> {
        let component = match Component::new(&config).map_err(|e| e.to_string())? {
            Some(c) => c,
            None => return Ok(None),
        };

        let (component, open_task) = open_capture_window(component, &config)?;

        let product = Self {
            phase: Phase::Capture(component),
            document: None,
            thumbnail_window: None,
            workspace_window: None,
            thumbnail_cursor: Point::ORIGIN,
        };

        Ok(Some((product, open_task)))
    }

    /// Auto-save the completed capture and enter the resulting phase. Keeps the
    /// `RgbaImage` in memory; never reloads from disk.
    pub fn apply_capture_completion(
        &mut self,
        image: RgbaImage,
        auto_save: Result<std::path::PathBuf, String>,
    ) {
        match select_presentation(Platform::Macos, auto_save) {
            Presentation::MacosSavedThumbnail(path) => {
                let handle = iced::widget::image::Handle::from_rgba(
                    image.width(),
                    image.height(),
                    image.as_raw().clone(),
                );
                self.document = Some(ResultDocument::saved(image, path.clone()));
                self.phase = Phase::Thumbnail(ThumbnailState::new(handle, path, Instant::now()));
            }
            Presentation::MacosUnsavedWorkspace(error) => {
                self.document = None;
                let workspace = ResultWorkspace::new(ResultDocument::unsaved(image), Some(error));
                self.phase = Phase::Workspace(workspace);
            }
            // Linux policy never reaches the macOS daemon.
            Presentation::LinuxSavedWorkspace(_) | Presentation::LinuxUnsavedWorkspace(_) => {
                unreachable!("macOS daemon received a Linux presentation");
            }
        }
    }

    /// Open the saved Result Workspace reusing the SAME in-memory document, so a
    /// thumbnail click never reloads the image. No-op if no saved document.
    pub fn open_workspace(&mut self) {
        if let Some(document) = self.document.take() {
            self.phase = Phase::Workspace(ResultWorkspace::new(document, None));
        }
    }

    pub fn workspace(&self) -> Option<&ResultWorkspace> {
        match &self.phase {
            Phase::Workspace(ws) => Some(ws),
            _ => None,
        }
    }
}

/// Open the overlay window sized/positioned from the component's resolved
/// geometry and record its id via `boot`. The geometry math lives in the
/// component ([`Component::overlay_window_layout`]); this applies the host's
/// window-settings policy.
fn open_capture_window(
    mut component: Component,
    _config: &OverlayConfig,
) -> Result<(Component, Task<Message>), String> {
    let (window_size, window_origin) = component
        .overlay_window_layout()
        .map_err(|e| e.to_string())?;

    let (overlay_window, open_overlay) =
        window::open(overlay_window_settings(window_size, window_origin));
    let boot_task = component.boot(overlay_window);

    let open_task = open_overlay
        .map(rollshot_iced_overlay::macos_capture::Message::overlay_window_ready)
        .map(Message::Capture);

    Ok((
        component,
        Task::batch([boot_task.map(Message::Capture), open_task]),
    ))
}

fn overlay_window_settings(window_size: iced::Size, window_origin: Point) -> window::Settings {
    window::Settings {
        size: window_size,
        position: window::Position::Specific(window_origin),
        decorations: false,
        transparent: true,
        level: window::Level::AlwaysOnTop,
        resizable: false,
        ..window::Settings::default()
    }
}

/// Compact floating window settings for the saved-capture thumbnail.
fn thumbnail_window_settings() -> window::Settings {
    window::Settings {
        size: Size::new(220.0, 180.0),
        decorations: false,
        transparent: true,
        level: window::Level::AlwaysOnTop,
        resizable: false,
        ..window::Settings::default()
    }
}

fn workspace_window_settings() -> window::Settings {
    window::Settings {
        size: Size::new(1100.0, 760.0),
        min_size: Some(Size::new(640.0, 420.0)),
        decorations: true,
        resizable: true,
        exit_on_close_request: false,
        ..window::Settings::default()
    }
}

// ---------------------------------------------------------------------------
// Daemon delegation: update / view / subscription / theme / style
// ---------------------------------------------------------------------------

/// Drive one message through the current phase. No phase ever spins its own
/// event loop — capture host effects, thumbnail interactions, and workspace
/// messages all resolve to `Task<Message>` fed back through this single daemon.
fn update(product: &mut MacosProduct, message: Message) -> Task<Message> {
    match message {
        Message::Capture(msg) => {
            let Phase::Capture(component) = &mut product.phase else {
                return Task::none();
            };
            match component.update(msg) {
                HostEffect::None => Task::none(),
                HostEffect::Task(task) => task.map(Message::Capture),
                HostEffect::Completed(result) => complete_capture(product, result),
                HostEffect::Cancelled => iced::exit(),
                HostEffect::Fatal(error) => {
                    eprintln!("{error}");
                    iced::exit()
                }
            }
        }
        Message::Workspace(msg) => {
            let Phase::Workspace(workspace) = &mut product.phase else {
                return Task::none();
            };
            result_workspace::update(workspace, msg).map(Message::Workspace)
        }
        Message::ThumbnailCursorMoved(position) => {
            product.thumbnail_cursor = position;
            Task::none()
        }
        Message::ThumbnailPressed => {
            if let Phase::Thumbnail(state) = &mut product.phase {
                state.press_origin = Some(product.thumbnail_cursor);
                state.dragging = false;
                state.timer.set_dragging(true, Instant::now());
            }
            Task::none()
        }
        Message::ThumbnailReleased => {
            let action = match &product.phase {
                Phase::Thumbnail(state) => {
                    let start = state.press_origin.unwrap_or(product.thumbnail_cursor);
                    release_action(start, product.thumbnail_cursor, state.dragging)
                }
                _ => return Task::none(),
            };
            if let Phase::Thumbnail(state) = &mut product.phase {
                state.press_origin = None;
                state.timer.set_dragging(false, Instant::now());
            }
            match action {
                ThumbnailAction::OpenWorkspace => open_thumbnail_workspace(product),
                // TODO(task-9): hand off to the native AppKit drag. For now the
                // thumbnail simply stays open; no objc2 drag is started here.
                ThumbnailAction::StartNativeDrag
                | ThumbnailAction::KeepOpen
                | ThumbnailAction::Close => Task::none(),
            }
        }
        Message::ThumbnailHoverChanged(hovering) => {
            if let Phase::Thumbnail(state) = &mut product.phase {
                state.timer.set_hovering(hovering, Instant::now());
            }
            Task::none()
        }
        Message::ThumbnailTick(now) => {
            let expired =
                matches!(&mut product.phase, Phase::Thumbnail(state) if state.timer.tick(now));
            if expired {
                iced::exit()
            } else {
                Task::none()
            }
        }
        Message::ThumbnailWindowReady(id) => {
            product.thumbnail_window = Some(id);
            Task::none()
        }
        Message::WorkspaceWindowReady(id) => {
            product.workspace_window = Some(id);
            Task::none()
        }
    }
}

/// Close the capture-owned windows, keep the captured image in memory, auto-save
/// it, select the macOS presentation, and open the resulting window — all inside
/// the one daemon.
fn complete_capture(product: &mut MacosProduct, result: CaptureResult) -> Task<Message> {
    let CaptureResult { image, .. } = result;

    // Close any capture-owned windows before switching phase.
    let mut close_tasks = Vec::new();
    if let Phase::Capture(component) = &mut product.phase {
        if let Some(id) = component.overlay_window() {
            close_tasks.push(window::close(id));
        }
        if let Some(id) = component.controls_window() {
            close_tasks.push(window::close(id));
        }
        component.shutdown();
    }

    let auto_save = storage::auto_save(&image, Platform::Macos);
    product.apply_capture_completion(image, auto_save);

    let open_task = match &product.phase {
        Phase::Thumbnail(_) => {
            let (id, open) = window::open(thumbnail_window_settings());
            product.thumbnail_window = Some(id);
            open.map(Message::ThumbnailWindowReady)
        }
        Phase::Workspace(_) => {
            let (id, open) = window::open(workspace_window_settings());
            product.workspace_window = Some(id);
            open.map(Message::WorkspaceWindowReady)
        }
        Phase::Capture(_) => Task::none(),
    };

    close_tasks.push(open_task);
    Task::batch(close_tasks)
}

/// Thumbnail click: close the thumbnail window and open the saved Result
/// Workspace reusing the in-memory document.
fn open_thumbnail_workspace(product: &mut MacosProduct) -> Task<Message> {
    let mut tasks = Vec::new();
    if let Some(id) = product.thumbnail_window.take() {
        tasks.push(window::close(id));
    }
    product.open_workspace();
    if matches!(product.phase, Phase::Workspace(_)) {
        let (id, open) = window::open(workspace_window_settings());
        product.workspace_window = Some(id);
        tasks.push(open.map(Message::WorkspaceWindowReady));
    }
    Task::batch(tasks)
}

fn view(product: &MacosProduct, window: window::Id) -> Element<'_, Message> {
    match &product.phase {
        Phase::Capture(component) if component.owns_window(window) => {
            component.view(window).map(Message::Capture)
        }
        Phase::Thumbnail(state) => macos_thumbnail::view(state),
        Phase::Workspace(workspace) => result_workspace::view(workspace).map(Message::Workspace),
        // A window event arriving for a phase that no longer owns it: render
        // nothing rather than panic.
        Phase::Capture(_) => iced::widget::container(iced::widget::Space::new()).into(),
    }
}

fn subscription(product: &MacosProduct) -> iced::Subscription<Message> {
    match &product.phase {
        Phase::Capture(component) => component.subscription().map(Message::Capture),
        Phase::Thumbnail(_) => {
            let tick = iced::time::every(std::time::Duration::from_millis(250))
                .map(|_| Message::ThumbnailTick(Instant::now()));
            // Track the cursor over the thumbnail window for press/drag math.
            let cursor = iced::event::listen_with(|event, _status, _window| match event {
                iced::Event::Mouse(iced::mouse::Event::CursorMoved { position }) => {
                    Some(Message::ThumbnailCursorMoved(position))
                }
                _ => None,
            });
            iced::Subscription::batch([tick, cursor])
        }
        Phase::Workspace(workspace) => {
            result_workspace::subscription(workspace).map(Message::Workspace)
        }
    }
}

fn theme(product: &MacosProduct, window: window::Id) -> iced::Theme {
    match &product.phase {
        Phase::Capture(component) => component.theme(window),
        _ => iced::Theme::Dark,
    }
}

fn style(product: &MacosProduct, theme: &iced::Theme) -> iced::theme::Style {
    match &product.phase {
        Phase::Capture(component) => component.style(theme),
        _ => iced::theme::Style {
            background_color: theme.palette().background,
            text_color: theme.palette().text,
        },
    }
}

/// Start exactly ONE `iced::daemon`, owning the whole post-capture flow.
pub fn run(config: OverlayConfig) -> Result<(), String> {
    use std::sync::Mutex;

    // The daemon `boot` closure is `Fn`, so it cannot own the non-`Clone`
    // `MacosProduct` directly. Acquire the capture component (which starts the
    // screen capture before the overlay surface exists) here and stash the built
    // product + boot task for the closure to take exactly once. `None` means the
    // user cancelled before any capture began, so no daemon is started.
    let (product, boot_task) = match MacosProduct::new(config)? {
        Some(pair) => pair,
        None => return Ok(()),
    };
    let slot: Mutex<Option<(MacosProduct, Task<Message>)>> = Mutex::new(Some((product, boot_task)));

    iced::daemon(
        move || {
            slot.lock()
                .unwrap()
                .take()
                .expect("product already taken by daemon boot")
        },
        update,
        view,
    )
    .subscription(subscription)
    .theme(theme)
    .style(style)
    .run()
    .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn image() -> RgbaImage {
        RgbaImage::from_pixel(4, 4, image::Rgba([10, 20, 30, 255]))
    }

    /// A product parked in the capture phase with no resources, used to drive
    /// completion transitions without a real capture.
    fn product_in_capture_phase() -> MacosProduct {
        let config = OverlayConfig {
            backend: "auto".to_string(),
            fps: 5,
            show_cursor: false,
            initial_mode: CaptureMode::Screenshot,
        };
        // `Component::new` uses test factories under cfg(test), so this builds a
        // bare component without touching real capture.
        let component = Component::new(&config)
            .expect("component new")
            .expect("test component");
        MacosProduct {
            phase: Phase::Capture(component),
            document: None,
            thumbnail_window: None,
            workspace_window: None,
            thumbnail_cursor: Point::ORIGIN,
        }
    }

    fn product_in_thumbnail_phase(image: RgbaImage, path: PathBuf) -> MacosProduct {
        let handle = iced::widget::image::Handle::from_rgba(
            image.width(),
            image.height(),
            image.as_raw().clone(),
        );
        MacosProduct {
            phase: Phase::Thumbnail(ThumbnailState::new(handle, path.clone(), Instant::now())),
            document: Some(ResultDocument::saved(image, path)),
            thumbnail_window: None,
            workspace_window: None,
            thumbnail_cursor: Point::ORIGIN,
        }
    }

    #[test]
    fn completed_capture_auto_save_success_enters_thumbnail() {
        let mut product = product_in_capture_phase();
        product.apply_capture_completion(image(), Ok(PathBuf::from("/tmp/cap.png")));
        assert!(matches!(product.phase, Phase::Thumbnail(_)));
        assert!(product.document.is_some());
    }

    #[test]
    fn completed_capture_auto_save_failure_enters_unsaved_workspace() {
        let mut product = product_in_capture_phase();
        product.apply_capture_completion(image(), Err("disk full".to_string()));
        match &product.phase {
            Phase::Workspace(ws) => {
                assert_eq!(ws.message_text().as_deref(), Some("disk full"));
            }
            _ => panic!("expected unsaved workspace phase"),
        }
        assert!(product.document.is_none());
    }

    #[test]
    fn thumbnail_click_enters_saved_workspace_without_reloading_image() {
        let img = image();
        let raw = img.as_raw().clone();
        let mut product = product_in_thumbnail_phase(img, PathBuf::from("/tmp/cap.png"));
        product.open_workspace();
        match &product.phase {
            Phase::Workspace(ws) => {
                // Same pixels reused from the in-memory document; not reloaded.
                assert_eq!(ws.document.source_image.as_raw(), &raw);
                assert!(ws.document.saved_path.is_some());
            }
            _ => panic!("expected saved workspace phase"),
        }
    }

    #[test]
    fn release_within_threshold_opens_workspace_from_thumbnail() {
        let mut product = product_in_thumbnail_phase(image(), PathBuf::from("/tmp/cap.png"));
        // Press then release at the same point (a click) → workspace.
        product.thumbnail_cursor = Point::new(5.0, 5.0);
        let _ = update(&mut product, Message::ThumbnailPressed);
        let _ = update(&mut product, Message::ThumbnailReleased);
        assert!(matches!(product.phase, Phase::Workspace(_)));
    }

    #[test]
    fn dragged_release_keeps_thumbnail_open() {
        let mut product = product_in_thumbnail_phase(image(), PathBuf::from("/tmp/cap.png"));
        product.thumbnail_cursor = Point::new(0.0, 0.0);
        let _ = update(&mut product, Message::ThumbnailPressed);
        // Move well past the drag threshold before releasing.
        product.thumbnail_cursor = Point::new(40.0, 0.0);
        let _ = update(&mut product, Message::ThumbnailReleased);
        // Native drag is a Task-9 placeholder; the thumbnail stays open.
        assert!(matches!(product.phase, Phase::Thumbnail(_)));
    }
}
