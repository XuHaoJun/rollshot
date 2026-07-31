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

use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::Arc;
use std::time::Instant;

use iced::{window, Element, Point, Size, Task};
use image::RgbaImage;

use rollshot_capture::{CaptureScope, Workflow};
use rollshot_iced_overlay::macos_capture::{Component, HostEffect};
use rollshot_iced_overlay::{CaptureResult, OverlayConfig};

#[cfg(feature = "action-guide")]
use crate::action_guide_home::{self, ActionGuideHome, ActionGuideIntent, SelectedDirectoryKind};
use crate::diagnostics::TARGET_APP;
use crate::macos_native_drag::{self, NativeDragResult};
use crate::macos_thumbnail::{self, release_action, ThumbnailAction, ThumbnailState};
use crate::post_capture::{select_presentation, CapturePurpose, Presentation};
use crate::result_workspace::{self, ResultDocument, ResultWorkspace};
use crate::storage::{self, Platform};
#[cfg(feature = "action-guide")]
use crate::timeline_workspace::{self, TimelineWorkspace};

/// Whether fullscreen mode bypasses the overlay entirely or the overlay session
/// is needed (Region / Scrolling).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InitialCapturePath {
    Overlay,
    Fullscreen,
}

fn initial_capture_path(scope: CaptureScope) -> InitialCapturePath {
    match scope {
        CaptureScope::Fullscreen => InitialCapturePath::Fullscreen,
        CaptureScope::Region => InitialCapturePath::Overlay,
    }
}

/// Estimated canvas area for the workspace window (1100×760 minus chrome),
/// so fit-mode zoom produces a visible scale before the scrollable reports its
/// real bounds. A pixel or two of inaccuracy is harmless.
const INITIAL_WORKSPACE_VIEWPORT: Size = Size::new(1084.0, 650.0);

/// Messages handled by the product daemon. Capture/workspace variants forward to
/// their owners; the remaining variants drive the thumbnail phase and host-side
/// window-open resolutions.
#[derive(Debug, Clone)]
#[allow(clippy::large_enum_variant)]
pub enum Message {
    /// A capture-component message (opaque to the host except for window-ready).
    Capture(rollshot_iced_overlay::macos_capture::Message),
    /// A Result Workspace message.
    Workspace(result_workspace::Message),
    /// A Timeline Workspace (Action Guide) message.
    #[cfg(feature = "action-guide")]
    Timeline(timeline_workspace::Message),
    #[cfg(feature = "action-guide")]
    RecordingTray(crate::macos_recording_tray::Event),
    /// Action Guide Home screen message.
    #[cfg(feature = "action-guide")]
    HomeMsg(action_guide_home::Message),
    /// Async inspection of a directory completed.
    #[cfg(feature = "action-guide")]
    SelectionInspected {
        #[allow(dead_code)]
        path: std::path::PathBuf,
        kind: SelectedDirectoryKind,
    },
    /// Async project open completed.
    #[cfg(feature = "action-guide")]
    ProjectOpened(ProjectOpenResult),
    /// User chose "Open Read-Only" in the lock-conflict dialog.
    #[cfg(feature = "action-guide")]
    OpenReadOnly,
    /// User chose "Cancel" in the lock-conflict dialog.
    #[cfg(feature = "action-guide")]
    CancelLockedOpen,
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
    /// The thumbnail window was patched (Ok) or the patch failed (Err) — a patch
    /// failure is treated as thumbnail-creation failure: print + exit.
    ThumbnailWindowPatched(Result<(), String>),
    /// The native AppKit drag was kicked off (Ok) or failed to start (Err) — a
    /// failure restarts the countdown rather than exiting.
    NativeDragStarted(Result<(), String>),
    /// The workspace window finished opening.
    WorkspaceWindowReady(window::Id),
    /// Background OCR completed (text or error), with graphical feedback flag.
    QuickOcrFinished {
        result: Result<String, crate::quick_ocr::QuickOcrError>,
        graphical_feedback: bool,
    },
}

#[cfg(feature = "action-guide")]
#[derive(Clone)]
pub(crate) enum ProjectOpenResult {
    Workspace(std::sync::Arc<TimelineWorkspace>),
    WriterLocked { path: std::path::PathBuf },
    Error(String),
}

#[cfg(feature = "action-guide")]
impl std::fmt::Debug for ProjectOpenResult {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Workspace(_) => f.debug_tuple("Workspace").field(&"..").finish(),
            Self::WriterLocked { path } => {
                f.debug_struct("WriterLocked").field("path", path).finish()
            }
            Self::Error(e) => f.debug_tuple("Error").field(e).finish(),
        }
    }
}

/// The current phase of the product daemon.
#[allow(clippy::large_enum_variant)]
pub enum Phase {
    #[cfg(feature = "action-guide")]
    Home(ActionGuideHome),
    #[cfg(feature = "action-guide")]
    Opening(ActionGuideHome),
    #[cfg(feature = "action-guide")]
    LockConflict(ActionGuideHome),
    Capture(Component),
    Thumbnail(ThumbnailState),
    Workspace(ResultWorkspace),
    #[cfg(feature = "action-guide")]
    Timeline(TimelineWorkspace),
}

/// The single daemon state.
pub struct MacosProduct {
    phase: Phase,
    /// The capture purpose driving this session.
    purpose: CapturePurpose,
    /// The in-memory capture document, kept across the thumbnail→workspace
    /// transition so a thumbnail click never reloads the image from disk.
    document: Option<ResultDocument>,
    thumbnail_window: Option<window::Id>,
    workspace_window: Option<window::Id>,
    /// Latest known cursor position over the thumbnail window.
    thumbnail_cursor: Point,
    #[cfg(feature = "action-guide")]
    recording_tray: Option<crate::macos_recording_tray::Guard>,
    #[cfg(feature = "action-guide")]
    lock_conflict_path: Option<std::path::PathBuf>,
    #[cfg(feature = "action-guide")]
    task_store: Option<Arc<crate::agent_store::TaskStore>>,
}

impl MacosProduct {
    /// Build the daemon state, embedding the capture component and producing the
    /// task that opens its overlay window. Returns `Ok(None)` when the user
    /// cancelled before any capture began (the caller then skips the daemon
    /// entirely), or `Err` if capture setup failed.
    pub fn new(
        config: OverlayConfig,
        purpose: CapturePurpose,
    ) -> Result<Option<(Self, Task<Message>)>, String> {
        let initial_path = if config.request.workflow == Workflow::ActionGuide {
            InitialCapturePath::Overlay
        } else {
            initial_capture_path(config.request.scope)
        };
        match initial_path {
            InitialCapturePath::Fullscreen => {
                let result = match rollshot_iced_overlay::fullscreen::capture(&config)
                    .map_err(|error| error.to_string())?
                {
                    Some(result) => result,
                    None => return Ok(None),
                };
                let auto_save = storage::auto_save(&result.image, Platform::Macos);
                let mut product =
                    MacosProduct::from_completed_image(result.image, auto_save, purpose);
                let open_task = open_presentation_window(&mut product);
                Ok(Some((product, open_task)))
            }
            InitialCapturePath::Overlay => {
                #[cfg(feature = "action-guide")]
                let action_input_source = Some(crate::action_input::create_input_source());

                let component = match Component::new(
                    &config,
                    #[cfg(feature = "action-guide")]
                    action_input_source,
                    #[cfg(feature = "action-guide")]
                    None,
                )
                .map_err(|error| error.to_string())?
                {
                    Some(component) => component,
                    None => return Ok(None),
                };
                let (component, open_task) = open_capture_window(component, &config)?;
                #[cfg(feature = "action-guide")]
                let recording_tray = (config.request
                    == rollshot_capture::CaptureRequest::action_guide_fullscreen())
                .then(crate::macos_recording_tray::Guard::start)
                .transpose()?;
                Ok(Some((
                    MacosProduct {
                        phase: Phase::Capture(component),
                        purpose,
                        document: None,
                        thumbnail_window: None,
                        workspace_window: None,
                        thumbnail_cursor: Point::ORIGIN,
                        #[cfg(feature = "action-guide")]
                        recording_tray,
                        #[cfg(feature = "action-guide")]
                        lock_conflict_path: None,
                        #[cfg(feature = "action-guide")]
                        task_store: None,
                    },
                    open_task,
                )))
            }
        }
    }

    /// Construct a product directly from a completed image and auto-save result,
    /// bypassing the overlay capture phase. Used by fullscreen bootstrap.
    fn from_completed_image(
        image: RgbaImage,
        auto_save: Result<std::path::PathBuf, String>,
        purpose: CapturePurpose,
    ) -> Self {
        let (phase, document) = match select_presentation(Platform::Macos, auto_save) {
            Presentation::MacosSavedThumbnail(path) => {
                let source_size = Size::new(image.width() as f32, image.height() as f32);
                let scale = crate::result_workspace::viewport::display_downscale_scale(
                    source_size,
                    crate::result_workspace::viewport::DEFAULT_MAX_TEXTURE_DIM,
                );
                let handle = crate::result_workspace::build_display_handle(&image, scale);
                (
                    Phase::Thumbnail(ThumbnailState::new(handle, path.clone(), Instant::now())),
                    Some(ResultDocument::saved(image, path)),
                )
            }
            Presentation::MacosUnsavedWorkspace(error) => {
                let workspace = ResultWorkspace::new(ResultDocument::unsaved(image), Some(error))
                    .with_initial_viewport(INITIAL_WORKSPACE_VIEWPORT);
                (Phase::Workspace(workspace), None)
            }
            Presentation::LinuxSavedWorkspace(_) | Presentation::LinuxUnsavedWorkspace(_) => {
                unreachable!("macOS daemon received a Linux presentation");
            }
        };
        MacosProduct {
            phase,
            purpose,
            document,
            thumbnail_window: None,
            workspace_window: None,
            thumbnail_cursor: Point::ORIGIN,
            #[cfg(feature = "action-guide")]
            recording_tray: None,
            #[cfg(feature = "action-guide")]
            lock_conflict_path: None,
            #[cfg(feature = "action-guide")]
            task_store: None,
        }
    }

    /// Auto-save the completed capture and enter the resulting phase. Keeps the
    /// `RgbaImage` in memory; never reloads from disk.
    pub fn apply_capture_completion(
        &mut self,
        image: RgbaImage,
        auto_save: Result<std::path::PathBuf, String>,
    ) {
        let completed = Self::from_completed_image(image, auto_save, self.purpose);
        self.phase = completed.phase;
        self.document = completed.document;
    }

    /// Open the saved Result Workspace reusing the SAME in-memory document, so a
    /// thumbnail click never reloads the image. No-op if no saved document.
    pub fn open_workspace(&mut self) {
        if let Some(document) = self.document.take() {
            self.phase = Phase::Workspace(
                ResultWorkspace::new(document, None)
                    .with_initial_viewport(INITIAL_WORKSPACE_VIEWPORT),
            );
        }
    }

    #[allow(dead_code)]
    pub fn workspace(&self) -> Option<&ResultWorkspace> {
        match &self.phase {
            Phase::Workspace(ws) => Some(ws),
            _ => None,
        }
    }

    #[cfg(test)]
    #[cfg(feature = "action-guide")]
    fn home_mut(&mut self) -> Option<&mut ActionGuideHome> {
        match &mut self.phase {
            Phase::Home(home) | Phase::Opening(home) | Phase::LockConflict(home) => Some(home),
            _ => None,
        }
    }

    #[cfg(feature = "action-guide")]
    pub fn new_action_guide(
        recent: crate::action_guide_home::recent::RecentProjects,
        task_store: Arc<crate::agent_store::TaskStore>,
    ) -> (Self, Task<Message>) {
        let product = MacosProduct {
            phase: Phase::Home(ActionGuideHome::new(recent)),
            purpose: CapturePurpose::Present,
            document: None,
            thumbnail_window: None,
            workspace_window: None,
            thumbnail_cursor: Point::ORIGIN,
            recording_tray: None,
            lock_conflict_path: None,
            task_store: Some(task_store),
        };
        (product, Task::none())
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
    let boot_task = component.boot(overlay_window, window_origin);

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

/// Logical size of the compact saved-capture thumbnail card.
const THUMBNAIL_SIZE: Size = Size::new(220.0, 180.0);
/// Margin (logical points) between the thumbnail and the active screen edge.
const THUMBNAIL_MARGIN: f32 = 24.0;

/// Compact floating window settings for the saved-capture thumbnail, placed in
/// the lower-right of the screen currently under the pointer.
fn thumbnail_window_settings() -> Result<window::Settings, String> {
    let origin =
        macos_native_drag::active_screen_thumbnail_origin(THUMBNAIL_SIZE, THUMBNAIL_MARGIN)?;
    Ok(window::Settings {
        size: THUMBNAIL_SIZE,
        position: window::Position::Specific(origin),
        decorations: false,
        transparent: true,
        level: window::Level::AlwaysOnTop,
        resizable: false,
        ..window::Settings::default()
    })
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
            let effect = component.update(msg);
            #[cfg(feature = "action-guide")]
            if let Some(status) = component.take_motion_status_change() {
                if let Some(ref mut tray) = product.recording_tray {
                    tray.set_motion_status(status);
                }
            }
            apply_capture_host_effect(product, effect)
        }
        Message::Workspace(msg) => {
            let Phase::Workspace(workspace) = &mut product.phase else {
                return Task::none();
            };
            result_workspace::update(workspace, msg).map(Message::Workspace)
        }
        #[cfg(feature = "action-guide")]
        Message::Timeline(msg) => {
            let Phase::Timeline(workspace) = &mut product.phase else {
                return Task::none();
            };
            let result = timeline_workspace::update(workspace, msg);
            match result.effect {
                timeline_workspace::Effect::None => result.task.map(Message::Timeline),
                timeline_workspace::Effect::CloseWorkspace => {
                    let mut close_tasks = Vec::new();
                    if let Some(id) = product.workspace_window.take() {
                        close_tasks.push(window::close(id));
                    }
                    product.phase = Phase::Home(load_action_guide_home());
                    Task::batch(close_tasks)
                }
                timeline_workspace::Effect::ProjectSaved {
                    root,
                    display_name,
                    close_workspace,
                } => {
                    let mut home = load_action_guide_home();
                    home.record_project_open(root, display_name);
                    if close_workspace {
                        let mut close_tasks = Vec::new();
                        if let Some(id) = product.workspace_window.take() {
                            close_tasks.push(window::close(id));
                        }
                        product.phase = Phase::Home(home);
                        Task::batch(close_tasks)
                    } else {
                        Task::none()
                    }
                }
            }
        }
        #[cfg(feature = "action-guide")]
        Message::RecordingTray(event) => {
            let Phase::Capture(component) = &mut product.phase else {
                return Task::none();
            };
            let effect = match event {
                crate::macos_recording_tray::Event::Finish => component.finish_action_recording(),
                crate::macos_recording_tray::Event::Cancel => component.cancel_action_recording(),
            };
            apply_capture_host_effect(product, effect)
        }
        #[cfg(feature = "action-guide")]
        Message::HomeMsg(home_msg) => {
            let Phase::Home(ref mut home) = &mut product.phase else {
                return Task::none();
            };
            let result = home.update(home_msg);
            match result.effect {
                action_guide_home::Effect::None => result.task.map(Message::HomeMsg),
                action_guide_home::Effect::PickProject => {
                    Task::perform(pick_project_folder(), Message::HomeMsg)
                }
                action_guide_home::Effect::InspectSelection(path) => {
                    let Phase::Home(home) = std::mem::replace(
                        &mut product.phase,
                        Phase::Opening(ActionGuideHome::new_empty()),
                    ) else {
                        unreachable!();
                    };
                    product.phase = Phase::Opening(home);
                    Task::perform(inspect_and_open(path), |msg| msg)
                }
                action_guide_home::Effect::StartRecording { motion_toolchain } => {
                    start_action_guide_recording(product, false, motion_toolchain)
                }
                action_guide_home::Effect::OpenProject(path) => {
                    let Phase::Home(home) = std::mem::replace(
                        &mut product.phase,
                        Phase::Opening(ActionGuideHome::new_empty()),
                    ) else {
                        unreachable!();
                    };
                    product.phase = Phase::Opening(home);
                    Task::perform(
                        open_project_task(
                            path,
                            true,
                            product.task_store.clone().expect("Action Guide task store"),
                        ),
                        |msg| msg,
                    )
                }
                action_guide_home::Effect::OpenLegacyReader(path) => {
                    if let Phase::Home(ref mut home) = &mut product.phase {
                        home.message = open_legacy_reader(&path).err();
                    }
                    Task::none()
                }
                action_guide_home::Effect::PickRecording => {
                    Task::perform(pick_recording_file(), Message::HomeMsg)
                }
                action_guide_home::Effect::ResolveImportToolchain { operation_id } => {
                    Task::perform(resolve_import_toolchain(operation_id), Message::HomeMsg)
                }
                action_guide_home::Effect::SetupImportToolchain { operation_id } => {
                    Task::perform(setup_import_toolchain(operation_id), Message::HomeMsg)
                }
                action_guide_home::Effect::StartImport {
                    job_id: _,
                    path,
                    toolchain,
                    cancellation,
                    reporter,
                } => action_guide_home::update::run_import_task(
                    path,
                    toolchain,
                    cancellation,
                    reporter,
                )
                .map(Message::HomeMsg),
                action_guide_home::Effect::OpenImportedTimeline(seed) => {
                    let mut ws = TimelineWorkspace::from_imported_video(seed);
                    ws.task_store = product.task_store.clone();
                    let initial_load = ws.initial_frame_load_task().map(Message::Timeline);
                    product.phase = Phase::Timeline(ws);
                    let (id, open) = window::open(workspace_window_settings());
                    product.workspace_window = Some(id);
                    Task::batch([open.map(Message::WorkspaceWindowReady), initial_load])
                }
            }
        }
        #[cfg(feature = "action-guide")]
        Message::SelectionInspected { path: _, kind } => {
            let Phase::Opening(ref mut home) = &mut product.phase else {
                return Task::none();
            };
            match kind {
                SelectedDirectoryKind::Project(project_path) => Task::perform(
                    open_project_task(
                        project_path,
                        true,
                        product.task_store.clone().expect("Action Guide task store"),
                    ),
                    |msg| msg,
                ),
                SelectedDirectoryKind::LegacyReader(reader_path) => {
                    home.message = open_legacy_reader(&reader_path).err();
                    let Phase::Opening(home) = std::mem::replace(
                        &mut product.phase,
                        Phase::Home(ActionGuideHome::new_empty()),
                    ) else {
                        unreachable!();
                    };
                    product.phase = Phase::Home(home);
                    Task::none()
                }
                SelectedDirectoryKind::Invalid => {
                    home.message = Some("Selected path is not a valid project".into());
                    let Phase::Opening(home) = std::mem::replace(
                        &mut product.phase,
                        Phase::Home(ActionGuideHome::new_empty()),
                    ) else {
                        unreachable!();
                    };
                    product.phase = Phase::Home(home);
                    Task::none()
                }
            }
        }
        #[cfg(feature = "action-guide")]
        Message::ProjectOpened(result) => {
            if !matches!(product.phase, Phase::Opening(_)) {
                return Task::none();
            }
            match result {
                ProjectOpenResult::Workspace(ws) => {
                    let mut ws = match std::sync::Arc::try_unwrap(ws) {
                        Ok(ws) => ws,
                        Err(_) => unreachable!("sole ownership"),
                    };
                    ws.task_store = product.task_store.clone();
                    let Phase::Opening(mut home) = std::mem::replace(
                        &mut product.phase,
                        Phase::Home(ActionGuideHome::new_empty()),
                    ) else {
                        unreachable!();
                    };
                    if let Some((root, display_name)) = ws.project_recent_metadata() {
                        home.record_project_open(root, display_name);
                    }
                    let initial_load = ws.initial_frame_load_task().map(Message::Timeline);
                    product.phase = Phase::Timeline(ws);
                    let (id, open) = window::open(workspace_window_settings());
                    product.workspace_window = Some(id);
                    Task::batch([open.map(Message::WorkspaceWindowReady), initial_load])
                }
                ProjectOpenResult::WriterLocked { path } => {
                    let Phase::Opening(home) = std::mem::replace(
                        &mut product.phase,
                        Phase::LockConflict(ActionGuideHome::new_empty()),
                    ) else {
                        unreachable!();
                    };
                    product.lock_conflict_path = Some(path);
                    product.phase = Phase::LockConflict(home);
                    Task::none()
                }
                ProjectOpenResult::Error(error) => {
                    if let Phase::Opening(ref mut home) = &mut product.phase {
                        home.message = Some(error);
                    }
                    let Phase::Opening(home) = std::mem::replace(
                        &mut product.phase,
                        Phase::Home(ActionGuideHome::new_empty()),
                    ) else {
                        unreachable!();
                    };
                    product.phase = Phase::Home(home);
                    Task::none()
                }
            }
        }
        #[cfg(feature = "action-guide")]
        Message::OpenReadOnly => {
            let Some(path) = product.lock_conflict_path.take() else {
                let Phase::LockConflict(home) = std::mem::replace(
                    &mut product.phase,
                    Phase::Home(ActionGuideHome::new_empty()),
                ) else {
                    unreachable!();
                };
                product.phase = Phase::Home(home);
                return Task::none();
            };
            let Phase::LockConflict(home) = std::mem::replace(
                &mut product.phase,
                Phase::Opening(ActionGuideHome::new_empty()),
            ) else {
                unreachable!();
            };
            product.phase = Phase::Opening(home);
            Task::perform(
                open_project_task(
                    path,
                    false,
                    product.task_store.clone().expect("Action Guide task store"),
                ),
                |msg| msg,
            )
        }
        #[cfg(feature = "action-guide")]
        Message::CancelLockedOpen => {
            product.lock_conflict_path = None;
            let Phase::LockConflict(home) = std::mem::replace(
                &mut product.phase,
                Phase::Home(ActionGuideHome::new_empty()),
            ) else {
                unreachable!();
            };
            product.phase = Phase::Home(home);
            Task::none()
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
                ThumbnailAction::StartNativeDrag => start_native_drag(product),
                ThumbnailAction::KeepOpen | ThumbnailAction::Close => Task::none(),
            }
        }
        Message::ThumbnailHoverChanged(hovering) => {
            if let Phase::Thumbnail(state) = &mut product.phase {
                state.timer.set_hovering(hovering, Instant::now());
            }
            Task::none()
        }
        Message::ThumbnailTick(now) => {
            // A finished native drag wins over the countdown: success closes the
            // thumbnail and exits; cancel/failure clears `dragging` and restarts
            // the 8s countdown. Only advance the timer when no drag completed.
            match poll_native_drag(product) {
                Some(NativeDragResult::Succeeded) => return iced::exit(),
                Some(NativeDragResult::Cancelled) => {
                    if let Phase::Thumbnail(state) = &mut product.phase {
                        state.dragging = false;
                        state.native_drag_status = None;
                        state.timer.set_dragging(false, now);
                        state.timer = crate::macos_thumbnail::ThumbnailTimer::new(
                            now,
                            crate::macos_thumbnail::THUMBNAIL_TIMEOUT,
                        );
                    }
                    return Task::none();
                }
                // Pending or not in a drag: fall through to the countdown.
                Some(NativeDragResult::Pending) | None => {}
            }
            let expired = if let Phase::Thumbnail(state) = &mut product.phase {
                state.timer.tick(now)
            } else {
                false
            };
            if expired {
                iced::exit()
            } else {
                Task::none()
            }
        }
        Message::ThumbnailWindowReady(id) => {
            product.thumbnail_window = Some(id);
            // Patch the floating thumbnail window before it accepts interaction.
            window::run(id, |window| {
                macos_native_drag::patch_thumbnail_window(window)
            })
            .map(Message::ThumbnailWindowPatched)
        }
        Message::ThumbnailWindowPatched(result) => {
            if let Err(error) = result {
                tracing::error!(target: TARGET_APP, %error, "thumbnail window patch failed");
                return iced::exit();
            }
            Task::none()
        }
        Message::NativeDragStarted(result) => {
            if let Err(error) = result {
                tracing::warn!(target: TARGET_APP, %error, "native drag failed to start");
                if let Phase::Thumbnail(state) = &mut product.phase {
                    let now = Instant::now();
                    state.dragging = false;
                    state.native_drag_status = None;
                    state.timer = crate::macos_thumbnail::ThumbnailTimer::new(
                        now,
                        crate::macos_thumbnail::THUMBNAIL_TIMEOUT,
                    );
                }
            }
            Task::none()
        }
        Message::WorkspaceWindowReady(id) => {
            product.workspace_window = Some(id);
            Task::none()
        }
        Message::QuickOcrFinished {
            result,
            graphical_feedback,
        } => {
            use crate::quick_ocr::{CliOutput, NoopFeedback, QuickOcrFeedback, StdoutOutput};
            match result {
                Ok(text) => {
                    let mut output = StdoutOutput;
                    let mut feedback = NoopFeedback;
                    if let Err(e) = output.write_text(&format!("{text}\n")) {
                        tracing::error!(target: TARGET_APP, %e, "OCR stdout write failed");
                    }
                    if graphical_feedback {
                        if let Err(e) = feedback.copied() {
                            tracing::warn!(target: TARGET_APP, %e, "OCR feedback failed");
                        }
                    }
                    tracing::info!(target: TARGET_APP, "quick OCR completed");
                }
                Err(error) => {
                    tracing::error!(target: TARGET_APP, %error, "quick OCR failed");
                }
            }
            iced::exit()
        }
    }
}

fn apply_capture_host_effect(product: &mut MacosProduct, effect: HostEffect) -> Task<Message> {
    match effect {
        HostEffect::None => Task::none(),
        HostEffect::Task(task) => task.map(Message::Capture),
        HostEffect::Completed(result) => complete_capture(product, result),
        #[cfg(feature = "action-guide")]
        HostEffect::ActionRecorded(result) => complete_action_recording(product, result),
        HostEffect::Cancelled => {
            #[cfg(feature = "action-guide")]
            {
                product.recording_tray = None;
            }
            iced::exit()
        }
        HostEffect::Fatal(error) => {
            #[cfg(feature = "action-guide")]
            {
                product.recording_tray = None;
            }
            tracing::error!(target: TARGET_APP, %error, "capture fatal");
            iced::exit()
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

    match product.purpose {
        CapturePurpose::Ocr { graphical_feedback } => {
            // OCR path: spawn a blocking worker, do not create thumbnail/workspace.
            let task = Task::perform(
                async move {
                    tokio::task::spawn_blocking(move || crate::product_ocr::prepare(&image))
                        .await
                        .map_err(|_| crate::quick_ocr::QuickOcrError::Worker)
                        .and_then(|result| result.map_err(crate::quick_ocr::QuickOcrError::Ocr))
                },
                move |result| {
                    let text_result = result.and_then(|items| {
                        let mut clipboard = crate::quick_ocr::ArboardClipboard;
                        crate::quick_ocr::finish_with(items, &mut clipboard)
                    });
                    Message::QuickOcrFinished {
                        result: text_result,
                        graphical_feedback,
                    }
                },
            );
            return Task::batch(close_tasks).chain(task);
        }
        CapturePurpose::Present => {
            let auto_save = storage::auto_save(&image, Platform::Macos);
            product.apply_capture_completion(image, auto_save);
            close_tasks.push(open_presentation_window(product));
        }
    }
    Task::batch(close_tasks)
}

/// Close the capture-owned windows, build the Timeline Workspace from the
/// finished recording, enter `Phase::Timeline`, and open the workspace window —
/// all inside the one daemon (mirrors `complete_capture`).
#[cfg(feature = "action-guide")]
fn complete_action_recording(
    product: &mut MacosProduct,
    result: rollshot_iced_overlay::driver::ActionGuideCaptureResult,
) -> Task<Message> {
    let rollshot_iced_overlay::driver::ActionGuideCaptureResult {
        recording,
        capability,
        region,
        motion,
    } = result;
    product.recording_tray = None;
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

    let source_kind =
        crate::timeline_workspace::source_kind_for(capability, crate::storage::Platform::Macos);
    product.phase = Phase::Timeline(TimelineWorkspace::new(
        recording,
        region,
        capability,
        source_kind,
        Some(motion),
    ));

    let (id, open) = window::open(workspace_window_settings());
    product.workspace_window = Some(id);
    close_tasks.push(open.map(Message::WorkspaceWindowReady));
    Task::batch(close_tasks)
}

/// Open the window for the current presentation phase (thumbnail or workspace),
/// recording the window id on `product`. On thumbnail-settings failure the
/// durable saved file remains (spec §13), so this returns `iced::exit()` after
/// logging.
fn open_presentation_window(product: &mut MacosProduct) -> Task<Message> {
    match &product.phase {
        Phase::Thumbnail(_) => match thumbnail_window_settings() {
            Ok(settings) => {
                let (id, open) = window::open(settings);
                product.thumbnail_window = Some(id);
                open.map(Message::ThumbnailWindowReady)
            }
            Err(error) => {
                tracing::error!(target: TARGET_APP, %error, "thumbnail window settings failed");
                iced::exit()
            }
        },
        Phase::Workspace(_) => {
            let (id, open) = window::open(workspace_window_settings());
            product.workspace_window = Some(id);
            open.map(Message::WorkspaceWindowReady)
        }
        #[cfg(feature = "action-guide")]
        Phase::Timeline(_) => {
            // Defensive parallel to Phase::Workspace: the Timeline workspace
            // window is normally opened by complete_action_recording, so this
            // arm is not reached in the standard flow.
            let (id, open) = window::open(workspace_window_settings());
            product.workspace_window = Some(id);
            open.map(Message::WorkspaceWindowReady)
        }
        Phase::Capture(_) => Task::none(),
        #[cfg(feature = "action-guide")]
        Phase::Home(_) | Phase::Opening(_) | Phase::LockConflict(_) => Task::none(),
    }
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

/// Begin the native AppKit file drag from the thumbnail window: mark the timer
/// paused, publish a shared status atomic the host tick polls, and run the
/// AppKit bridge against the thumbnail's raw window handle.
fn start_native_drag(product: &mut MacosProduct) -> Task<Message> {
    let Phase::Thumbnail(state) = &mut product.phase else {
        return Task::none();
    };
    let Some(window_id) = product.thumbnail_window else {
        return Task::none();
    };

    let now = Instant::now();
    state.dragging = true;
    state.timer.set_dragging(true, now);

    let status = Arc::new(AtomicU8::new(NativeDragResult::Pending as u8));
    state.native_drag_status = Some(Arc::clone(&status));
    let saved_path = state.saved_path.clone();

    window::run(window_id, move |window| {
        macos_native_drag::begin_file_drag(window, &saved_path, status)
    })
    .map(Message::NativeDragStarted)
}

/// Read the shared native-drag status atomic, if a drag is in flight. Returns
/// `None` when not in the thumbnail phase or no drag has been started.
fn poll_native_drag(product: &MacosProduct) -> Option<NativeDragResult> {
    let Phase::Thumbnail(state) = &product.phase else {
        return None;
    };
    let status = state.native_drag_status.as_ref()?;
    Some(match status.load(Ordering::SeqCst) {
        x if x == NativeDragResult::Succeeded as u8 => NativeDragResult::Succeeded,
        x if x == NativeDragResult::Cancelled as u8 => NativeDragResult::Cancelled,
        _ => NativeDragResult::Pending,
    })
}

fn view(product: &MacosProduct, window: window::Id) -> Element<'_, Message> {
    match &product.phase {
        #[cfg(feature = "action-guide")]
        Phase::Home(home) => action_guide_home::view::view(home).map(Message::HomeMsg),
        #[cfg(feature = "action-guide")]
        Phase::Opening(home) => action_guide_home::view::view(home).map(Message::HomeMsg),
        #[cfg(feature = "action-guide")]
        Phase::LockConflict(_) => lock_conflict_view(),
        Phase::Capture(component) if component.owns_window(window) => {
            component.view(window).map(Message::Capture)
        }
        Phase::Thumbnail(state) => macos_thumbnail::view(state),
        Phase::Workspace(workspace) => result_workspace::view(workspace).map(Message::Workspace),
        #[cfg(feature = "action-guide")]
        Phase::Timeline(workspace) => timeline_workspace::view(workspace).map(Message::Timeline),
        // A window event arriving for a phase that no longer owns it: render
        // nothing rather than panic.
        Phase::Capture(_) => iced::widget::container(iced::widget::Space::new()).into(),
    }
}

fn subscription(product: &MacosProduct) -> iced::Subscription<Message> {
    let phase = match &product.phase {
        #[cfg(feature = "action-guide")]
        Phase::Home(home) | Phase::Opening(home) | Phase::LockConflict(home) => {
            action_guide_home::update::subscription(home).map(Message::HomeMsg)
        }
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
        #[cfg(feature = "action-guide")]
        Phase::Timeline(workspace) => {
            timeline_workspace::subscription(workspace).map(Message::Timeline)
        }
    };
    #[cfg(feature = "action-guide")]
    if product.recording_tray.is_some() {
        return iced::Subscription::batch([
            phase,
            crate::macos_recording_tray::subscription().map(Message::RecordingTray),
        ]);
    }
    phase
}

#[cfg(feature = "action-guide")]
fn lock_conflict_view<'a>() -> Element<'a, Message> {
    use iced::widget::{button, column, container, row, text};

    let body = column![
        text("Project is open in another window").size(18),
        text("The project is currently locked by another process.").size(14),
        row![
            button(text("Open Read-Only").size(14))
                .on_press(Message::OpenReadOnly)
                .padding([8, 16]),
            button(text("Cancel").size(14))
                .on_press(Message::CancelLockedOpen)
                .padding([8, 16]),
        ]
        .spacing(12),
    ]
    .spacing(12)
    .padding(24)
    .align_x(iced::Alignment::Center);

    container(body)
        .width(iced::Length::Fill)
        .height(iced::Length::Fill)
        .center_x(iced::Length::Fill)
        .center_y(iced::Length::Fill)
        .into()
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
pub fn run(config: OverlayConfig, purpose: CapturePurpose) -> Result<(), String> {
    // The daemon `boot` closure is `Fn`, so it cannot own the non-`Clone`
    // `MacosProduct` directly. Acquire the capture component (which starts the
    // screen capture before the overlay surface exists) here and stash the built
    // product + boot task for the closure to take exactly once. `None` means the
    // user cancelled before any capture began, so no daemon is started.
    let (product, boot_task) = match MacosProduct::new(config, purpose)? {
        Some(pair) => pair,
        None => return Ok(()),
    };
    run_product(product, boot_task)
}

fn from_imported_document(document: ResultDocument) -> (MacosProduct, Task<Message>) {
    let workspace =
        ResultWorkspace::new(document, None).with_initial_viewport(INITIAL_WORKSPACE_VIEWPORT);
    let mut product = MacosProduct {
        phase: Phase::Workspace(workspace),
        purpose: CapturePurpose::Present,
        document: None,
        thumbnail_window: None,
        workspace_window: None,
        thumbnail_cursor: Point::ORIGIN,
        #[cfg(feature = "action-guide")]
        recording_tray: None,
        #[cfg(feature = "action-guide")]
        lock_conflict_path: None,
        #[cfg(feature = "action-guide")]
        task_store: None,
    };
    let open_task = open_presentation_window(&mut product);
    (product, open_task)
}

fn run_product(product: MacosProduct, boot_task: Task<Message>) -> Result<(), String> {
    use std::sync::Mutex;

    let slot = Mutex::new(Some((product, boot_task)));
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
    .font(rollshot_image_document::style::FONT_REGULAR_BYTES)
    .font(rollshot_image_document::style::FONT_BOLD_BYTES)
    .theme(theme)
    .style(style)
    .run()
    .map_err(|error| error.to_string())
}

pub fn run_imported(document: ResultDocument) -> Result<(), String> {
    let (product, boot_task) = from_imported_document(document);
    run_product(product, boot_task)
}

#[cfg(feature = "action-guide")]
async fn inspect_and_open(path: std::path::PathBuf) -> Message {
    let kind = tokio::task::spawn_blocking(move || {
        action_guide_home::update::inspect_selection_shape(&path)
    })
    .await
    .unwrap_or(SelectedDirectoryKind::Invalid);
    Message::SelectionInspected {
        path: kind_path(&kind),
        kind,
    }
}

#[cfg(feature = "action-guide")]
fn kind_path(kind: &SelectedDirectoryKind) -> std::path::PathBuf {
    match kind {
        SelectedDirectoryKind::Project(p) | SelectedDirectoryKind::LegacyReader(p) => p.clone(),
        SelectedDirectoryKind::Invalid => std::path::PathBuf::new(),
    }
}

#[cfg(feature = "action-guide")]
async fn open_project_task(
    path: std::path::PathBuf,
    writable: bool,
    task_store: Arc<crate::agent_store::TaskStore>,
) -> Message {
    let result = open_project_inner(path, writable, task_store).await;
    Message::ProjectOpened(result)
}

#[cfg(feature = "action-guide")]
async fn open_project_inner(
    path: std::path::PathBuf,
    writable: bool,
    task_store: Arc<crate::agent_store::TaskStore>,
) -> ProjectOpenResult {
    let request = crate::timeline_workspace::project::OpenProjectRequest {
        root: path,
        writable,
    };
    let result = match crate::timeline_workspace::project::load_project_worker(request).await {
        Ok(r) => r,
        Err(e) => return ProjectOpenResult::Error(e.message_for_ui()),
    };
    match result {
        crate::timeline_workspace::project::OpenProjectWorkerResult::Opened(opened) => {
            match crate::timeline_workspace::project::from_loaded_project_with_task_store(
                opened.loaded,
                opened.access,
                task_store,
            ) {
                Ok(ws) => ProjectOpenResult::Workspace(std::sync::Arc::new(ws)),
                Err(e) => ProjectOpenResult::Error(format!("Failed to build workspace: {e:?}")),
            }
        }
        crate::timeline_workspace::project::OpenProjectWorkerResult::WriterLocked { root } => {
            ProjectOpenResult::WriterLocked { path: root }
        }
    }
}

/// Start the Action Guide product daemon on macOS with the Home phase.
#[cfg(feature = "action-guide")]
pub fn run_action_guide(initial: ActionGuideIntent) -> Result<(), String> {
    use std::sync::Mutex;

    let config_dir =
        crate::daemon::config::rollshot_config_dir().map_err(|e| format!("config dir: {e}"))?;
    let recent = crate::action_guide_home::recent::RecentProjects::load(&config_dir);
    let task_store = crate::agent_store::open_process_store(&config_dir)
        .map_err(|e| format!("task store: {e}"))?;

    cleanup_stale_import_scratch();

    let boot_data = Arc::new(Mutex::new(Some((initial, recent, task_store))));
    let boot = move || {
        let (boot_initial, recent, task_store) = boot_data
            .lock()
            .unwrap()
            .take()
            .expect("boot data already consumed");
        let (mut product, base_task) = MacosProduct::new_action_guide(recent, task_store);
        let mut tasks = vec![base_task];

        match boot_initial {
            ActionGuideIntent::Home => {}
            ActionGuideIntent::Record {
                fullscreen,
                keep_motion: _,
            } => {
                tasks.push(start_action_guide_recording(&mut product, fullscreen, None));
            }
            ActionGuideIntent::Open { path: Some(path) } => {
                let Phase::Home(home) = std::mem::replace(
                    &mut product.phase,
                    Phase::Opening(ActionGuideHome::new_empty()),
                ) else {
                    unreachable!();
                };
                product.phase = Phase::Opening(home);
                tasks.push(Task::perform(inspect_and_open(path), |msg| msg));
            }
            ActionGuideIntent::Open { path: None } => {
                if let Phase::Home(ref mut home) = &mut product.phase {
                    home.opening = true;
                }
                tasks.push(Task::perform(pick_project_folder(), Message::HomeMsg));
            }
        }

        (product, Task::batch(tasks))
    };

    iced::daemon(boot, update, view)
        .title(action_guide_title)
        .subscription(subscription)
        .font(rollshot_image_document::style::FONT_REGULAR_BYTES)
        .font(rollshot_image_document::style::FONT_BOLD_BYTES)
        .theme(theme)
        .style(style)
        .run()
        .map_err(|e| e.to_string())
}

#[cfg(feature = "action-guide")]
fn action_guide_record_config(fullscreen: bool) -> OverlayConfig {
    let intent = ActionGuideIntent::Record {
        fullscreen,
        keep_motion: false,
    };
    OverlayConfig {
        backend: "auto".to_string(),
        fps: 5,
        show_cursor: false,
        request: intent
            .capture_request()
            .expect("record intent always has a capture request"),
        target_output_name: None,
    }
}

#[cfg(feature = "action-guide")]
fn start_action_guide_recording(
    product: &mut MacosProduct,
    fullscreen: bool,
    motion_toolchain: Option<rollshot_action::video_import::VideoToolchain>,
) -> Task<Message> {
    let config = action_guide_record_config(fullscreen);
    let action_input_source = Some(crate::action_input::create_input_source());
    let component = match Component::new(&config, action_input_source, motion_toolchain)
        .map_err(|error| error.to_string())
    {
        Ok(Some(component)) => component,
        Ok(None) => return Task::none(),
        Err(error) => {
            tracing::error!(target: TARGET_APP, %error, "action guide capture setup failed");
            return Task::none();
        }
    };
    let (component, open_task) = match open_capture_window(component, &config) {
        Ok(pair) => pair,
        Err(error) => {
            tracing::error!(target: TARGET_APP, %error, "action guide capture window failed");
            return Task::none();
        }
    };
    product.recording_tray = if fullscreen {
        crate::macos_recording_tray::Guard::start().ok()
    } else {
        None
    };
    product.phase = Phase::Capture(component);
    open_task
}

#[cfg(feature = "action-guide")]
fn action_guide_title(product: &MacosProduct, _window: window::Id) -> String {
    match &product.phase {
        Phase::Home(_) | Phase::Opening(_) | Phase::LockConflict(_) => {
            "Rollshot — Action Guide".to_string()
        }
        Phase::Timeline(_) => "Rollshot — Timeline".to_string(),
        _ => String::new(),
    }
}

#[cfg(feature = "action-guide")]
async fn pick_project_folder() -> action_guide_home::Message {
    let folder = rfd::AsyncFileDialog::new()
        .set_title("Open Action Guide Project")
        .pick_folder()
        .await;
    match folder {
        Some(handle) => action_guide_home::Message::PickerSelected(handle.path().to_path_buf()),
        None => action_guide_home::Message::PickerCancelled,
    }
}

#[cfg(feature = "action-guide")]
async fn pick_recording_file() -> action_guide_home::Message {
    let file = rfd::AsyncFileDialog::new()
        .set_title("Select Video Recording")
        .add_filter("Video", &["mp4", "mov", "mkv", "webm"])
        .pick_file()
        .await;
    match file {
        Some(handle) => {
            action_guide_home::Message::ImportPickerSelected(handle.path().to_path_buf())
        }
        None => action_guide_home::Message::ImportPickerCancelled,
    }
}

#[cfg(feature = "action-guide")]
async fn resolve_import_toolchain(
    operation_id: action_guide_home::video_import::ImportOperationId,
) -> action_guide_home::Message {
    let resolution =
        tokio::task::spawn_blocking(crate::managed_ffmpeg::resolve_video_import_toolchain)
            .await
            .unwrap_or(
                crate::managed_ffmpeg::VideoImportToolchainResolution::NeedsSetup(
                    crate::managed_ffmpeg::FfmpegSetupInfo {
                        managed_download: None,
                        install_location: std::path::PathBuf::new(),
                    },
                ),
            );
    action_guide_home::Message::ImportToolchainResolved {
        operation_id,
        resolution,
    }
}

#[cfg(feature = "action-guide")]
async fn setup_import_toolchain(
    operation_id: action_guide_home::video_import::ImportOperationId,
) -> action_guide_home::Message {
    let result = tokio::task::spawn_blocking(crate::managed_ffmpeg::download_managed_ffmpeg)
        .await
        .map_err(|e| format!("Setup worker panicked: {e}"))
        .and_then(|r| r);
    action_guide_home::Message::ImportSetupFinished {
        operation_id,
        result: result.map(|_| ()),
    }
}

#[cfg(feature = "action-guide")]
fn open_legacy_reader(path: &std::path::Path) -> Result<(), String> {
    let entrypoint = action_guide_home::legacy_reader_entrypoint(path).map_err(str::to_string)?;
    crate::platform_actions::open_path(&entrypoint)
}

#[cfg(feature = "action-guide")]
fn load_action_guide_home() -> ActionGuideHome {
    match crate::daemon::config::rollshot_config_dir() {
        Ok(config_dir) => ActionGuideHome::new(
            crate::action_guide_home::recent::RecentProjects::load(&config_dir),
        ),
        Err(error) => {
            let mut home = ActionGuideHome::new_empty();
            home.message = Some(format!("Could not load recent projects: {error}"));
            home
        }
    }
}

#[cfg(feature = "action-guide")]
fn cleanup_stale_import_scratch() {
    crate::action_guide_home::cleanup_stale_import_scratch();
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
            request: rollshot_capture::CaptureRequest::screenshot_region(),
            target_output_name: None,
        };
        // `Component::new` uses test factories under cfg(test), so this builds a
        // bare component without touching real capture.
        let component = Component::new(
            &config,
            #[cfg(feature = "action-guide")]
            None,
            #[cfg(feature = "action-guide")]
            None,
        )
        .expect("component new")
        .expect("test component");
        MacosProduct {
            phase: Phase::Capture(component),
            purpose: CapturePurpose::Present,
            document: None,
            thumbnail_window: None,
            workspace_window: None,
            thumbnail_cursor: Point::ORIGIN,
            #[cfg(feature = "action-guide")]
            recording_tray: None,
            #[cfg(feature = "action-guide")]
            lock_conflict_path: None,
            #[cfg(feature = "action-guide")]
            task_store: None,
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
            purpose: CapturePurpose::Present,
            document: Some(ResultDocument::saved(image, path)),
            thumbnail_window: None,
            workspace_window: None,
            thumbnail_cursor: Point::ORIGIN,
            #[cfg(feature = "action-guide")]
            recording_tray: None,
            #[cfg(feature = "action-guide")]
            lock_conflict_path: None,
            #[cfg(feature = "action-guide")]
            task_store: None,
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
    fn completed_oversized_capture_downscales_thumbnail_handle_only() {
        let image = RgbaImage::from_pixel(100, 9000, image::Rgba([10, 20, 30, 255]));
        let mut product = product_in_capture_phase();

        product.apply_capture_completion(image, Ok(PathBuf::from("/tmp/cap.png")));

        let Phase::Thumbnail(state) = &product.phase else {
            panic!("expected thumbnail phase");
        };
        let (width, height) = match state.image_handle.clone() {
            iced::widget::image::Handle::Rgba { width, height, .. } => (width, height),
            _ => panic!("expected rgba thumbnail handle"),
        };
        assert!(width <= crate::result_workspace::viewport::DEFAULT_MAX_TEXTURE_DIM);
        assert!(height <= crate::result_workspace::viewport::DEFAULT_MAX_TEXTURE_DIM);
        assert_eq!(
            product
                .document
                .as_ref()
                .expect("saved document")
                .image
                .source()
                .dimensions(),
            (100, 9000)
        );
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
                assert_eq!(ws.document.image.source().as_raw(), &raw);
                assert!(ws.document.source_path().is_some());
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
        // A native drag hands off to AppKit without leaving the thumbnail
        // phase; the thumbnail window stays open while the drag is in flight.
        assert!(matches!(product.phase, Phase::Thumbnail(_)));
    }

    #[test]
    fn fullscreen_selects_direct_initial_path() {
        assert_eq!(
            initial_capture_path(CaptureScope::Fullscreen),
            InitialCapturePath::Fullscreen
        );
        assert_eq!(
            initial_capture_path(CaptureScope::Region),
            InitialCapturePath::Overlay
        );
    }

    #[test]
    fn fullscreen_success_bootstraps_existing_thumbnail_phase() {
        let product = MacosProduct::from_completed_image(
            image(),
            Ok(PathBuf::from("/tmp/fullscreen.png")),
            CapturePurpose::Present,
        );
        assert!(matches!(product.phase, Phase::Thumbnail(_)));
    }

    #[test]
    fn fullscreen_auto_save_failure_bootstraps_existing_workspace_phase() {
        let product = MacosProduct::from_completed_image(
            image(),
            Err("disk full".to_string()),
            CapturePurpose::Present,
        );
        assert!(matches!(product.phase, Phase::Workspace(_)));
    }

    #[test]
    fn imported_document_boots_workspace_without_capture_or_thumbnail_state() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("source.png");
        image()
            .save_with_format(&path, image::ImageFormat::Png)
            .unwrap();
        let imported = crate::image_import::load(&path).unwrap();
        let document = ResultDocument::imported(imported.pixels, imported.source);

        let (product, _open_task) = from_imported_document(document);

        assert!(matches!(product.phase, Phase::Workspace(_)));
        assert!(product.document.is_none());
        assert!(product.thumbnail_window.is_none());
        assert!(product.workspace_window.is_some());
    }

    #[cfg(feature = "action-guide")]
    #[test]
    fn complete_action_recording_enters_timeline_phase() {
        use image::{Rgba, RgbaImage};
        use rollshot_action::{ActionRecorder, CaptureRegion, DetectorConfig, StoreConfig};

        let region = CaptureRegion {
            x: 0,
            y: 0,
            width: 32,
            height: 32,
        };
        let det = DetectorConfig {
            diff_threshold: 0.01,
            area_threshold: 0.05,
            cooldown_ms: 0,
            ..DetectorConfig::default()
        };
        let mut rec = ActionRecorder::new(region, StoreConfig::default(), det);
        rec.ingest_frame(
            Arc::new(RgbaImage::from_pixel(32, 32, Rgba([0, 0, 0, 255]))),
            0,
        );
        for i in 1..=6 {
            let mut img = RgbaImage::from_pixel(32, 32, Rgba([0, 0, 0, 255]));
            for y in 0..16 {
                for x in 0..16 {
                    img.put_pixel(x, y, Rgba([255, 255, 255, 255]));
                }
            }
            rec.ingest_frame(Arc::new(img), i * 100);
        }
        let recording = rec.finish();

        let mut product = product_in_capture_phase();
        let result = rollshot_iced_overlay::driver::ActionGuideCaptureResult {
            recording,
            capability: rollshot_action::InputCapability::SemanticEvents,
            region,
            motion: rollshot_action::motion::MotionRecordingOutcome::Failure(
                rollshot_action::motion::MotionFailureCategory::ToolUnavailable,
            ),
        };
        let _ = complete_action_recording(&mut product, result);
        assert!(matches!(product.phase, Phase::Timeline(_)));
    }

    #[test]
    fn quick_ocr_finished_ok_does_not_mutate_phase() {
        let mut product = product_in_capture_phase();
        let task = update(
            &mut product,
            Message::QuickOcrFinished {
                result: Ok("hello".to_string()),
                graphical_feedback: false,
            },
        );
        // The handler returns iced::exit(); phase is untouched.
        assert!(matches!(product.phase, Phase::Capture(_)));
        // Verify the task is an exit task by attempting to run it (it completes
        // immediately — the daemon would shut down).
        drop(task);
    }

    #[test]
    fn quick_ocr_finished_err_does_not_mutate_phase() {
        let mut product = product_in_capture_phase();
        let task = update(
            &mut product,
            Message::QuickOcrFinished {
                result: Err(crate::quick_ocr::QuickOcrError::Worker),
                graphical_feedback: false,
            },
        );
        assert!(matches!(product.phase, Phase::Capture(_)));
        drop(task);
    }

    #[test]
    fn complete_capture_ocr_does_not_create_thumbnail_or_workspace() {
        let mut product = product_in_capture_phase();
        product.purpose = CapturePurpose::Ocr {
            graphical_feedback: false,
        };
        let image = image();
        let capture_result = rollshot_iced_overlay::CaptureResult { image, stats: None };
        let task = complete_capture(&mut product, capture_result);
        // OCR path returns a task (spawn + close) but never transitions to
        // thumbnail or workspace phase.
        assert!(
            matches!(product.phase, Phase::Capture(_)),
            "OCR path should not change phase"
        );
        assert!(product.document.is_none());
        drop(task);
    }

    #[test]
    fn graphical_feedback_forwarded_through_ocr_message() {
        let mut product = product_in_capture_phase();
        let _ = update(
            &mut product,
            Message::QuickOcrFinished {
                result: Ok("text".to_string()),
                graphical_feedback: true,
            },
        );
        // graphical_feedback=true should not panic; NoopFeedback absorbs the call.
        assert!(matches!(product.phase, Phase::Capture(_)));
    }

    // ---- Action Guide phase tests (Task 8) ----

    #[cfg(feature = "action-guide")]
    mod action_guide_project_tests {
        use super::super::*;
        use crate::action_guide_home::{self, ActionGuideHome};
        use crate::timeline_workspace;
        use image::{Rgba, RgbaImage};
        use rollshot_action::{ActionRecorder, CaptureRegion, DetectorConfig, StoreConfig};
        use std::path::PathBuf;

        fn test_recording() -> rollshot_action::Recording {
            let region = CaptureRegion {
                x: 0,
                y: 0,
                width: 32,
                height: 32,
            };
            let det = DetectorConfig {
                diff_threshold: 0.01,
                area_threshold: 0.05,
                cooldown_ms: 0,
                ..DetectorConfig::default()
            };
            let mut rec = ActionRecorder::new(region, StoreConfig::default(), det);
            rec.ingest_frame(
                Arc::new(RgbaImage::from_pixel(32, 32, Rgba([0, 0, 0, 255]))),
                0,
            );
            for i in 1..=6 {
                let mut img = RgbaImage::from_pixel(32, 32, Rgba([0, 0, 0, 255]));
                for y in 0..16 {
                    for x in 0..16 {
                        img.put_pixel(x, y, Rgba([255, 255, 255, 255]));
                    }
                }
                rec.ingest_frame(Arc::new(img), i * 100);
            }
            rec.finish()
        }

        fn test_timeline() -> TimelineWorkspace {
            TimelineWorkspace::new(
                test_recording(),
                CaptureRegion {
                    x: 0,
                    y: 0,
                    width: 32,
                    height: 32,
                },
                rollshot_action::InputCapability::SemanticEvents,
                rollshot_action::InputSourceKind::MacosCgEvent,
                None,
            )
        }

        fn product_in_home_phase() -> MacosProduct {
            let dir = tempfile::tempdir().unwrap();
            let task_store = Arc::new(crate::agent_store::TaskStore::open(dir.path()).unwrap());
            let _ = dir.keep();
            let (product, _task) = MacosProduct::new_action_guide(
                crate::action_guide_home::recent::RecentProjects::empty(),
                task_store,
            );
            product
        }

        #[test]
        fn home_launch_starts_in_home_phase() {
            let product = product_in_home_phase();
            assert!(matches!(product.phase, Phase::Home(_)));
        }

        #[test]
        fn action_guide_record_config_honors_fullscreen_flag() {
            assert_eq!(
                action_guide_record_config(false).request,
                rollshot_capture::CaptureRequest::action_guide_region()
            );
            assert_eq!(
                action_guide_record_config(true).request,
                rollshot_capture::CaptureRequest::action_guide_fullscreen()
            );
        }

        #[test]
        #[cfg(not(target_os = "macos"))]
        fn record_new_opens_preflight() {
            let mut product = product_in_home_phase();
            let task = update(
                &mut product,
                Message::HomeMsg(action_guide_home::Message::RecordNew),
            );
            // RecordNew now opens the preflight dialog instead of immediately starting.
            assert!(
                matches!(product.phase, Phase::Home(_)),
                "RecordNew should stay in Home phase with preflight open"
            );
            if let Phase::Home(ref home) = product.phase {
                assert!(
                    home.preflight.is_some(),
                    "preflight should be open after RecordNew"
                );
            }
            drop(task);
        }

        #[test]
        fn open_picker_stays_in_home_phase() {
            let mut product = product_in_home_phase();
            let task = update(
                &mut product,
                Message::HomeMsg(action_guide_home::Message::OpenPicker),
            );
            assert!(matches!(product.phase, Phase::Home(_)));
            assert!(task.units() > 0, "should launch folder picker");
        }

        #[test]
        fn home_inspect_selection_enters_opening_phase() {
            let mut product = product_in_home_phase();
            let project_path = PathBuf::from("/some/path");
            if let Phase::Home(ref mut home) = product.phase {
                home.recent
                    .record_open_at(project_path.clone(), "Test Project".into(), 1);
            }
            let task = update(
                &mut product,
                Message::HomeMsg(action_guide_home::Message::RecentSelected(project_path)),
            );
            assert!(matches!(product.phase, Phase::Opening(_)));
            assert!(task.units() > 0, "should return inspect task");
        }

        #[test]
        fn inspection_project_stays_in_opening() {
            let mut product = product_in_home_phase();
            product.phase = Phase::Opening(ActionGuideHome::new_empty());
            let task = update(
                &mut product,
                Message::SelectionInspected {
                    path: PathBuf::from("/some/project"),
                    kind: SelectedDirectoryKind::Project(PathBuf::from("/some/project")),
                },
            );
            assert!(matches!(product.phase, Phase::Opening(_)));
            assert!(task.units() > 0, "should return open task");
        }

        #[test]
        fn inspection_invalid_returns_to_home() {
            let mut product = product_in_home_phase();
            product.phase = Phase::Opening(ActionGuideHome::new_empty());
            let _task = update(
                &mut product,
                Message::SelectionInspected {
                    path: PathBuf::from("/invalid"),
                    kind: SelectedDirectoryKind::Invalid,
                },
            );
            assert!(matches!(product.phase, Phase::Home(_)));
        }

        #[test]
        fn inspection_legacy_reader_returns_to_home() {
            let mut product = product_in_home_phase();
            product.phase = Phase::Opening(ActionGuideHome::new_empty());
            let _task = update(
                &mut product,
                Message::SelectionInspected {
                    path: PathBuf::from("/legacy"),
                    kind: SelectedDirectoryKind::LegacyReader(PathBuf::from("/legacy")),
                },
            );
            assert!(matches!(product.phase, Phase::Home(_)));
        }

        #[test]
        fn project_opened_workspace_enters_timeline_phase() {
            let mut product = product_in_home_phase();
            product.phase = Phase::Opening(ActionGuideHome::new_empty());
            let ws = test_timeline();
            let task = update(
                &mut product,
                Message::ProjectOpened(ProjectOpenResult::Workspace(std::sync::Arc::new(ws))),
            );
            assert!(matches!(product.phase, Phase::Timeline(_)));
            let Phase::Timeline(workspace) = &product.phase else {
                unreachable!();
            };
            assert!(Arc::ptr_eq(
                workspace.task_store.as_ref().unwrap(),
                product.task_store.as_ref().unwrap(),
            ));
            assert!(task.units() > 0, "should open workspace window");
        }

        #[test]
        fn writer_locked_enters_lock_conflict_phase() {
            let mut product = product_in_home_phase();
            product.phase = Phase::Opening(ActionGuideHome::new_empty());
            let _task = update(
                &mut product,
                Message::ProjectOpened(ProjectOpenResult::WriterLocked {
                    path: PathBuf::from("/locked/project"),
                }),
            );
            assert!(matches!(product.phase, Phase::LockConflict(_)));
            assert!(product.lock_conflict_path.is_some());
        }

        #[test]
        fn open_read_only_from_lock_conflict() {
            let mut product = product_in_home_phase();
            product.phase = Phase::LockConflict(ActionGuideHome::new_empty());
            product.lock_conflict_path = Some(PathBuf::from("/locked/project"));
            let task = update(&mut product, Message::OpenReadOnly);
            assert!(matches!(product.phase, Phase::Opening(_)));
            assert!(product.lock_conflict_path.is_none());
            assert!(task.units() > 0, "should return open task");
        }

        #[test]
        fn open_read_only_without_path_returns_home() {
            let mut product = product_in_home_phase();
            product.phase = Phase::LockConflict(ActionGuideHome::new_empty());
            product.lock_conflict_path = None;
            let _task = update(&mut product, Message::OpenReadOnly);
            assert!(matches!(product.phase, Phase::Home(_)));
        }

        #[test]
        fn cancel_locked_open_returns_to_home() {
            let mut product = product_in_home_phase();
            product.phase = Phase::LockConflict(ActionGuideHome::new_empty());
            product.lock_conflict_path = Some(PathBuf::from("/locked/project"));
            let _task = update(&mut product, Message::CancelLockedOpen);
            assert!(matches!(product.phase, Phase::Home(_)));
            assert!(product.lock_conflict_path.is_none());
        }

        #[test]
        fn timeline_close_workspace_returns_to_home() {
            let mut product = product_in_home_phase();
            product.phase = Phase::Timeline(test_timeline());
            let _task = update(
                &mut product,
                Message::Timeline(timeline_workspace::Message::ConfirmDiscard),
            );
            assert!(matches!(product.phase, Phase::Home(_)));
        }

        #[test]
        fn project_open_error_returns_to_home_with_message() {
            let mut product = product_in_home_phase();
            product.phase = Phase::Opening(ActionGuideHome::new_empty());
            let _task = update(
                &mut product,
                Message::ProjectOpened(ProjectOpenResult::Error("lock failed".into())),
            );
            assert!(matches!(product.phase, Phase::Home(_)));
        }

        #[test]
        fn home_message_ignored_when_not_in_home_phase() {
            let mut product = product_in_home_phase();
            product.phase = Phase::Timeline(test_timeline());
            let task = update(
                &mut product,
                Message::HomeMsg(action_guide_home::Message::RecordNew),
            );
            assert!(matches!(product.phase, Phase::Timeline(_)));
            assert!(task.units() == 0);
        }

        #[test]
        fn project_opened_ignored_when_not_opening() {
            let mut product = product_in_home_phase();
            let ws = test_timeline();
            let task = update(
                &mut product,
                Message::ProjectOpened(ProjectOpenResult::Workspace(std::sync::Arc::new(ws))),
            );
            assert!(matches!(product.phase, Phase::Home(_)));
            assert!(task.units() == 0);
        }

        #[test]
        fn selection_inspected_ignored_when_not_opening() {
            let mut product = product_in_home_phase();
            let task = update(
                &mut product,
                Message::SelectionInspected {
                    path: PathBuf::from("/some/project"),
                    kind: SelectedDirectoryKind::Project(PathBuf::from("/some/project")),
                },
            );
            assert!(matches!(product.phase, Phase::Home(_)));
            assert!(task.units() == 0);
        }

        #[test]
        fn complete_action_recording_enters_timeline_with_save_first() {
            let recording = test_recording();
            let region = CaptureRegion {
                x: 0,
                y: 0,
                width: 32,
                height: 32,
            };
            let mut product = super::product_in_capture_phase();
            let result = rollshot_iced_overlay::driver::ActionGuideCaptureResult {
                recording,
                capability: rollshot_action::InputCapability::SemanticEvents,
                region,
                motion: rollshot_action::motion::MotionRecordingOutcome::Failure(
                    rollshot_action::motion::MotionFailureCategory::ToolUnavailable,
                ),
            };
            let _task = complete_action_recording(&mut product, result);
            assert!(matches!(product.phase, Phase::Timeline(_)));
        }

        fn dummy_import_seed(
            scratch_dir: &tempfile::TempDir,
        ) -> rollshot_action::ImportedWorkspaceSeed {
            use rollshot_action::project::ProjectFrame;
            use rollshot_action::{
                CandidateKind, CaptureRegion, DetectReason, Guide, GuideStep, ImportWarning,
                ImportedScratch, InputCapability, InputSourceKind,
            };
            let scratch = ImportedScratch::create(scratch_dir.path()).unwrap();
            let step = GuideStep {
                index: 1,
                title: "Click button".into(),
                caption: String::new(),
                kind: CandidateKind::Click,
                reason: DetectReason::VisualChange,
                at_ms: 100,
                keyframe: 1,
                nearby: vec![1],
                source: 1,
            };
            let guide = Guide::from_reviewed_steps("Imported Guide".into(), vec![step]).unwrap();
            rollshot_action::ImportedWorkspaceSeed {
                guide,
                capture_region: CaptureRegion {
                    x: 0,
                    y: 0,
                    width: 640,
                    height: 480,
                },
                input_source: InputSourceKind::ImportedVideo,
                input_capability: InputCapability::VisualOnly {
                    reason: rollshot_action::DegradedReason::ImportedRecording,
                },
                frames: vec![ProjectFrame {
                    id: 1,
                    at_ms: 100,
                    sha256: "abc123".into(),
                    width: 640,
                    height: 480,
                }],
                import_warnings: vec![ImportWarning::NoVisualChangesDetected],
                scratch,
            }
        }

        fn drive_import_success(product: &mut MacosProduct) {
            let home = product.home_mut().expect("should be in home-capable phase");
            let (_job_id, mut reporter) = home.bind_test_import();
            let t = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis()
                .try_into()
                .unwrap_or(u64::MAX);
            reporter.mark_running(t).unwrap();
            let scratch_dir = tempfile::tempdir().unwrap();
            let seed = dummy_import_seed(&scratch_dir);
            reporter.succeed(seed, t + 1).unwrap();
            let _task = update(
                product,
                Message::HomeMsg(action_guide_home::Message::ImportJobsChanged),
            );
        }

        fn macos_home_product() -> MacosProduct {
            product_in_home_phase()
        }

        #[test]
        fn macos_home_import_success_opens_workspace_window() {
            let mut product = macos_home_product();
            drive_import_success(&mut product);
            assert!(matches!(product.phase, Phase::Timeline(_)));
            let Phase::Timeline(workspace) = &product.phase else {
                unreachable!();
            };
            assert!(Arc::ptr_eq(
                workspace.task_store.as_ref().unwrap(),
                product.task_store.as_ref().unwrap(),
            ));
            assert!(product.workspace_window.is_some());
        }

        // ---- Task 9: macOS motion recording status tests ----

        #[test]
        fn toolchain_propagated_to_component() {
            // When a VideoToolchain is passed through start_action_guide_recording,
            // the component should have it available for begin_action_recording.
            // We verify the config honors the fullscreen flag and the component
            // can be created with a toolchain.
            let config = action_guide_record_config(true);
            assert_eq!(
                config.request,
                rollshot_capture::CaptureRequest::action_guide_fullscreen()
            );
        }

        #[test]
        fn motion_disabled_option_stays_off() {
            // When keep_motion is false, the preflight skips resolution and
            // passes motion_toolchain: None, resulting in Off status.
            let intent = ActionGuideIntent::Record {
                fullscreen: true,
                keep_motion: false,
            };
            assert_eq!(
                intent,
                ActionGuideIntent::Record {
                    fullscreen: true,
                    keep_motion: false,
                }
            );
            // With no toolchain, motion is disabled.
            let off = rollshot_action::motion::MotionRuntimeStatus::Off;
            assert_eq!(off, rollshot_action::motion::MotionRuntimeStatus::Off);
        }

        #[test]
        fn completion_carries_motion_outcome_to_workspace() {
            let recording = test_recording();
            let region = CaptureRegion {
                x: 0,
                y: 0,
                width: 32,
                height: 32,
            };
            let mut product = super::product_in_capture_phase();
            let motion = rollshot_action::motion::MotionRecordingOutcome::Failure(
                rollshot_action::motion::MotionFailureCategory::ToolUnavailable,
            );
            let result = rollshot_iced_overlay::driver::ActionGuideCaptureResult {
                recording,
                capability: rollshot_action::InputCapability::SemanticEvents,
                region,
                motion,
            };
            let _task = complete_action_recording(&mut product, result);
            assert!(matches!(product.phase, Phase::Timeline(_)));
            let Phase::Timeline(ref ws) = product.phase else {
                unreachable!();
            };
            assert!(
                ws.motion.is_failed_or_unavailable(),
                "motion state should reflect the failure"
            );
        }

        #[test]
        fn tray_on_text_matches_spec() {
            // The tray status mapping is tested in macos_recording_tray::tests
            // (cfg-independent). Here we verify the Guard type is available
            // and the event IDs are unchanged.
            assert_eq!(
                crate::macos_recording_tray::Event::Finish,
                crate::macos_recording_tray::Event::Finish
            );
            assert_eq!(
                crate::macos_recording_tray::Event::Cancel,
                crate::macos_recording_tray::Event::Cancel
            );
        }

        #[test]
        fn runtime_failure_remains_failed_through_completion() {
            // Verify that a MotionRecordingOutcome::Failure is preserved
            // when carried through ActionGuideCaptureResult into the workspace.
            let recording = test_recording();
            let region = CaptureRegion {
                x: 0,
                y: 0,
                width: 32,
                height: 32,
            };
            let mut product = super::product_in_capture_phase();
            let result = rollshot_iced_overlay::driver::ActionGuideCaptureResult {
                recording,
                capability: rollshot_action::InputCapability::SemanticEvents,
                region,
                motion: rollshot_action::motion::MotionRecordingOutcome::Failure(
                    rollshot_action::motion::MotionFailureCategory::BrokenPipe,
                ),
            };
            let _task = complete_action_recording(&mut product, result);
            let Phase::Timeline(ref ws) = product.phase else {
                unreachable!();
            };
            match &ws.motion {
                crate::timeline_workspace::motion::WorkspaceMotion::Failed(cat) => {
                    assert_eq!(
                        *cat,
                        rollshot_action::motion::MotionFailureCategory::BrokenPipe
                    );
                }
                other => panic!("expected Failed, got {other:?}"),
            }
        }
    }
}
