#![allow(dead_code)]

/// Linux Action Guide product host — single iced daemon with Home and Timeline
/// phases sharing one decorated window.
///
/// ```text
///  ┌─────────────────────────────────────────────────────────────┐
///  │                    iced::daemon                              │
///  │  ┌─────────┐   ┌──────────┐   ┌─────────────┐              │
///  │  │  Home    │──►│ Opening  │──►│  Timeline   │              │
///  │  │(default) │   │(async    │   │(workspace)  │              │
///  │  │          │◄──│ load)    │   │             │              │
///  │  └─────────┘   └──────────┘   └──────┬──────┘              │
///  │      ▲                                │ CloseWorkspace      │
///  │      └────────────────────────────────┘                     │
///  │      │                                                       │
///  │      │   ┌─────────────┐                                    │
///  │      └──►│ LockConflict│ (WriterLocked)                     │
///  │          │ Open RO/    │                                    │
///  │          │ Cancel      │                                    │
///  │          └─────────────┘                                    │
///  └─────────────────────────────────────────────────────────────┘
/// ```
///
/// Dormant until Task 9 wires launch routing. `run()` is the entry point.
use crate::action_guide_home::{self, ActionGuideHome, ActionGuideIntent, SelectedDirectoryKind};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Phase {
    Home,
    Opening,
    LockConflict,
    Timeline,
}

#[derive(Debug, Clone)]
pub(crate) enum Message {
    Home(action_guide_home::Message),
    Timeline(crate::timeline_workspace::Message),
    SelectionInspected {
        path: std::path::PathBuf,
        kind: SelectedDirectoryKind,
    },
    ProjectOpened(ProjectOpenResult),
    OpenReadOnly,
    CancelLockedOpen,
    WindowReady,
}

#[derive(Clone)]
pub(crate) enum ProjectOpenResult {
    Workspace(std::sync::Arc<crate::timeline_workspace::TimelineWorkspace>),
    WriterLocked { path: std::path::PathBuf },
    Error(String),
}

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

pub(crate) struct State {
    phase: Phase,
    home: ActionGuideHome,
    timeline: Option<crate::timeline_workspace::TimelineWorkspace>,
    lock_conflict_path: Option<std::path::PathBuf>,
}

impl State {
    fn new(recent: crate::action_guide_home::recent::RecentProjects) -> Self {
        Self {
            phase: Phase::Home,
            home: ActionGuideHome::new(recent),
            timeline: None,
            lock_conflict_path: None,
        }
    }
}

fn update(state: &mut State, message: Message) -> iced::Task<Message> {
    match message {
        Message::Home(home_msg) => {
            if state.phase != Phase::Home {
                return iced::Task::none();
            }
            let result = state.home.update(home_msg);
            match result.effect {
                action_guide_home::Effect::None => result.task.map(Message::Home),
                action_guide_home::Effect::PickProject => {
                    iced::Task::perform(pick_project_folder(), Message::Home)
                }
                action_guide_home::Effect::InspectSelection(path) => {
                    state.phase = Phase::Opening;
                    iced::Task::perform(inspect_and_open(path), |msg| msg)
                }
                action_guide_home::Effect::RecordNew => {
                    let _ = crate::platform_actions::spawn_action_guide_record(false);
                    iced::Task::none()
                }
                action_guide_home::Effect::OpenProject(path) => {
                    state.phase = Phase::Opening;
                    iced::Task::perform(open_project_task(path, true), |msg| msg)
                }
                action_guide_home::Effect::OpenLegacyReader(path) => {
                    state.home.message = open_legacy_reader(&path).err();
                    iced::Task::none()
                }
            }
        }
        Message::Timeline(tl_msg) => {
            let Some(ref mut ws) = state.timeline else {
                return iced::Task::none();
            };
            let result = crate::timeline_workspace::update(ws, tl_msg);
            match result.effect {
                crate::timeline_workspace::Effect::None => result.task.map(Message::Timeline),
                crate::timeline_workspace::Effect::CloseWorkspace => {
                    state.timeline = None;
                    state.phase = Phase::Home;
                    iced::Task::none()
                }
                crate::timeline_workspace::Effect::ProjectSaved {
                    root,
                    display_name,
                    close_workspace,
                } => {
                    state.home.record_project_open(root, display_name);
                    if close_workspace {
                        state.timeline = None;
                        state.phase = Phase::Home;
                    }
                    iced::Task::none()
                }
            }
        }
        Message::SelectionInspected { path: _, kind } => {
            if state.phase != Phase::Opening {
                return iced::Task::none();
            }
            match kind {
                SelectedDirectoryKind::Project(project_path) => {
                    iced::Task::perform(open_project_task(project_path, true), |msg| msg)
                }
                SelectedDirectoryKind::LegacyReader(reader_path) => {
                    state.phase = Phase::Home;
                    state.home.message = open_legacy_reader(&reader_path).err();
                    iced::Task::none()
                }
                SelectedDirectoryKind::Invalid => {
                    state.phase = Phase::Home;
                    state.home.opening = false;
                    state.home.message = Some("Selected path is not a valid project".into());
                    iced::Task::none()
                }
            }
        }
        Message::ProjectOpened(result) => {
            if state.phase != Phase::Opening {
                return iced::Task::none();
            }
            match result {
                ProjectOpenResult::Workspace(ws) => {
                    let ws = match std::sync::Arc::try_unwrap(ws) {
                        Ok(ws) => ws,
                        Err(_) => unreachable!("sole ownership"),
                    };
                    if let Some((root, display_name)) = ws.project_recent_metadata() {
                        state.home.record_project_open(root, display_name);
                    }
                    let initial_load = ws.initial_frame_load_task().map(Message::Timeline);
                    state.timeline = Some(ws);
                    state.phase = Phase::Timeline;
                    state.home.opening = false;
                    initial_load
                }
                ProjectOpenResult::WriterLocked { path } => {
                    state.lock_conflict_path = Some(path);
                    state.phase = Phase::LockConflict;
                    state.home.opening = false;
                    iced::Task::none()
                }
                ProjectOpenResult::Error(error) => {
                    state.phase = Phase::Home;
                    state.home.opening = false;
                    state.home.message = Some(error);
                    iced::Task::none()
                }
            }
        }
        Message::OpenReadOnly => {
            let Some(path) = state.lock_conflict_path.take() else {
                state.phase = Phase::Home;
                return iced::Task::none();
            };
            state.phase = Phase::Opening;
            iced::Task::perform(open_project_task(path, false), |msg| msg)
        }
        Message::CancelLockedOpen => {
            state.lock_conflict_path = None;
            state.phase = Phase::Home;
            iced::Task::none()
        }
        Message::WindowReady => iced::Task::none(),
    }
}

fn view(state: &State, _window: iced::window::Id) -> iced::Element<'_, Message> {
    match state.phase {
        Phase::Home | Phase::Opening => {
            action_guide_home::view::view(&state.home).map(Message::Home)
        }
        Phase::LockConflict => lock_conflict_view(),
        Phase::Timeline => {
            if let Some(ref ws) = state.timeline {
                crate::timeline_workspace::view(ws).map(Message::Timeline)
            } else {
                iced::widget::text("No timeline loaded").into()
            }
        }
    }
}

fn lock_conflict_view<'a>() -> iced::Element<'a, Message> {
    use iced::widget::{button, column, container, text};

    let body = column![
        text("Project is open in another window").size(18),
        text("The project is currently locked by another process.").size(14),
        iced::widget::row![
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

fn subscription(state: &State) -> iced::Subscription<Message> {
    match state.phase {
        Phase::Home | Phase::Opening | Phase::LockConflict => {
            crate::action_guide_home::update::subscription().map(Message::Home)
        }
        Phase::Timeline => {
            if let Some(ref ws) = state.timeline {
                crate::timeline_workspace::subscription(ws).map(Message::Timeline)
            } else {
                iced::Subscription::none()
            }
        }
    }
}

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

fn kind_path(kind: &SelectedDirectoryKind) -> std::path::PathBuf {
    match kind {
        SelectedDirectoryKind::Project(p) | SelectedDirectoryKind::LegacyReader(p) => p.clone(),
        SelectedDirectoryKind::Invalid => std::path::PathBuf::new(),
    }
}

async fn open_project_task(path: std::path::PathBuf, writable: bool) -> Message {
    let result = open_project_inner(path, writable).await;
    Message::ProjectOpened(result)
}

async fn open_project_inner(path: std::path::PathBuf, writable: bool) -> ProjectOpenResult {
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
            match crate::timeline_workspace::project::from_loaded_project(
                opened.loaded,
                opened.access,
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

pub(crate) fn run(initial: ActionGuideIntent) -> Result<(), String> {
    let config_dir =
        crate::daemon::config::rollshot_config_dir().map_err(|e| format!("config dir: {e}"))?;
    let recent = crate::action_guide_home::recent::RecentProjects::load(&config_dir);

    let boot_data = std::sync::Arc::new(std::sync::Mutex::new(Some((initial, recent))));
    let boot = move || {
        let (boot_initial, recent) = boot_data
            .lock()
            .unwrap()
            .take()
            .expect("boot data already consumed");
        let mut state = State::new(recent);
        let mut tasks = Vec::new();

        let (_window_id, open_task) = iced::window::open(iced::window::Settings {
            size: iced::Size::new(1100.0, 760.0),
            min_size: Some(iced::Size::new(640.0, 420.0)),
            decorations: true,
            resizable: true,
            exit_on_close_request: true,
            ..Default::default()
        });
        tasks.push(open_task.map(|_id| Message::WindowReady));

        match boot_initial {
            ActionGuideIntent::Home => {}
            ActionGuideIntent::Record { fullscreen } => {
                let _ = crate::platform_actions::spawn_action_guide_record(fullscreen);
            }
            ActionGuideIntent::Open { path: Some(path) } => {
                state.phase = Phase::Opening;
                tasks.push(iced::Task::perform(inspect_and_open(path), |msg| msg));
            }
            ActionGuideIntent::Open { path: None } => {
                state.home.opening = true;
                tasks.push(iced::Task::perform(pick_project_folder(), Message::Home));
            }
        }

        (state, iced::Task::batch(tasks))
    };

    iced::daemon(boot, update, view)
        .title(title)
        .font(rollshot_image_document::style::FONT_REGULAR_BYTES)
        .font(rollshot_image_document::style::FONT_BOLD_BYTES)
        .subscription(subscription)
        .run()
        .map_err(|e| e.to_string())
}

fn title(state: &State, _window: iced::window::Id) -> String {
    match state.phase {
        Phase::Home | Phase::Opening | Phase::LockConflict => "Rollshot — Action Guide".to_string(),
        Phase::Timeline => "Rollshot — Timeline".to_string(),
    }
}

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

fn open_legacy_reader(path: &std::path::Path) -> Result<(), String> {
    let entrypoint = action_guide_home::legacy_reader_entrypoint(path).map_err(str::to_string)?;
    crate::platform_actions::open_path(&entrypoint)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::action_guide_home::recent::RecentProjects;
    use std::path::PathBuf;

    fn test_state() -> State {
        let dir = tempfile::tempdir().unwrap();
        let config_dir = dir.path().join("rollshot");
        std::fs::create_dir_all(&config_dir).unwrap();
        let recent = RecentProjects::load(&config_dir);
        State::new(recent)
    }

    #[test]
    fn initial_phase_is_home() {
        let state = test_state();
        assert_eq!(state.phase, Phase::Home);
        assert!(state.timeline.is_none());
        assert!(state.lock_conflict_path.is_none());
    }

    #[test]
    fn record_new_spawns_child_and_stays_home() {
        let mut state = test_state();
        let task = update(
            &mut state,
            Message::Home(action_guide_home::Message::RecordNew),
        );
        assert_eq!(state.phase, Phase::Home);
        assert!(task.units() == 0, "RecordNew should not return a task");
    }

    #[test]
    fn open_picker_emits_pick_project_effect() {
        let mut state = test_state();
        let task = update(
            &mut state,
            Message::Home(action_guide_home::Message::OpenPicker),
        );
        assert_eq!(state.phase, Phase::Home);
        assert!(task.units() > 0, "should launch folder picker");
    }

    #[test]
    fn inspection_project_transitions_to_opening() {
        let mut state = test_state();
        state.phase = Phase::Opening;
        let task = update(
            &mut state,
            Message::SelectionInspected {
                path: PathBuf::from("/some/project"),
                kind: SelectedDirectoryKind::Project(PathBuf::from("/some/project")),
            },
        );
        assert_eq!(state.phase, Phase::Opening);
        assert!(task.units() > 0, "should return open task");
    }

    #[test]
    fn inspection_invalid_returns_to_home() {
        let mut state = test_state();
        state.phase = Phase::Opening;
        let task = update(
            &mut state,
            Message::SelectionInspected {
                path: PathBuf::from("/invalid"),
                kind: SelectedDirectoryKind::Invalid,
            },
        );
        assert_eq!(state.phase, Phase::Home);
        assert!(state.home.message.is_some());
        assert!(task.units() == 0);
    }

    #[test]
    fn inspection_legacy_reader_returns_to_home() {
        let mut state = test_state();
        state.phase = Phase::Opening;
        let task = update(
            &mut state,
            Message::SelectionInspected {
                path: PathBuf::from("/legacy"),
                kind: SelectedDirectoryKind::LegacyReader(PathBuf::from("/legacy")),
            },
        );
        assert_eq!(state.phase, Phase::Home);
        assert!(state.home.message.is_some());
        assert!(task.units() == 0);
    }

    fn test_recording() -> rollshot_action::Recording {
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
        rec.ingest_frame(RgbaImage::from_pixel(32, 32, Rgba([0, 0, 0, 255])), 0);
        for i in 1..=6 {
            let mut img = RgbaImage::from_pixel(32, 32, Rgba([0, 0, 0, 255]));
            for y in 0..16 {
                for x in 0..16 {
                    img.put_pixel(x, y, Rgba([255, 255, 255, 255]));
                }
            }
            rec.ingest_frame(img, i * 100);
        }
        rec.finish()
    }

    fn test_timeline() -> crate::timeline_workspace::TimelineWorkspace {
        crate::timeline_workspace::TimelineWorkspace::new(
            test_recording(),
            rollshot_action::CaptureRegion {
                x: 0,
                y: 0,
                width: 32,
                height: 32,
            },
            rollshot_action::InputCapability::SemanticEvents,
            rollshot_action::InputSourceKind::LinuxEvdev,
        )
    }

    #[test]
    fn project_opened_workspace_transitions_to_timeline() {
        let mut state = test_state();
        state.phase = Phase::Opening;

        let ws = test_timeline();
        let task = update(
            &mut state,
            Message::ProjectOpened(ProjectOpenResult::Workspace(std::sync::Arc::new(ws))),
        );
        assert_eq!(state.phase, Phase::Timeline);
        assert!(state.timeline.is_some());
        assert!(!state.home.opening);
        assert!(task.units() == 0);
    }

    #[test]
    fn project_opened_workspace_records_recent_project() {
        let mut state = test_state();
        state.phase = Phase::Opening;
        let root = PathBuf::from("/tmp/recent-project.rollshot-guide");
        let mut ws = test_timeline();
        ws.project_session = Some(crate::timeline_workspace::project::ProjectSession::Saved {
            root: root.clone(),
            base_revision: 1,
            access: crate::timeline_workspace::project::ProjectAccess::ReadOnly,
        });

        let _ = update(
            &mut state,
            Message::ProjectOpened(ProjectOpenResult::Workspace(std::sync::Arc::new(ws))),
        );

        assert_eq!(state.home.recent.entries()[0].path, root);
    }

    #[test]
    fn writer_locked_transitions_to_lock_conflict() {
        let mut state = test_state();
        state.phase = Phase::Opening;

        let task = update(
            &mut state,
            Message::ProjectOpened(ProjectOpenResult::WriterLocked {
                path: PathBuf::from("/locked/project"),
            }),
        );
        assert_eq!(state.phase, Phase::LockConflict);
        assert!(state.lock_conflict_path.is_some());
        assert!(!state.home.opening);
        assert!(task.units() == 0);
    }

    #[test]
    fn open_read_only_from_lock_conflict() {
        let mut state = test_state();
        state.phase = Phase::LockConflict;
        state.lock_conflict_path = Some(PathBuf::from("/locked/project"));

        let task = update(&mut state, Message::OpenReadOnly);
        assert_eq!(state.phase, Phase::Opening);
        assert!(state.lock_conflict_path.is_none());
        assert!(task.units() > 0, "should return open task");
    }

    #[test]
    fn open_read_only_without_path_returns_home() {
        let mut state = test_state();
        state.phase = Phase::LockConflict;
        state.lock_conflict_path = None;

        let task = update(&mut state, Message::OpenReadOnly);
        assert_eq!(state.phase, Phase::Home);
        assert!(task.units() == 0);
    }

    #[test]
    fn cancel_locked_open_returns_to_home() {
        let mut state = test_state();
        state.phase = Phase::LockConflict;
        state.lock_conflict_path = Some(PathBuf::from("/locked/project"));

        let task = update(&mut state, Message::CancelLockedOpen);
        assert_eq!(state.phase, Phase::Home);
        assert!(state.lock_conflict_path.is_none());
        assert!(task.units() == 0);
    }

    #[test]
    fn timeline_close_workspace_returns_to_home() {
        let mut state = test_state();
        state.phase = Phase::Timeline;
        state.timeline = Some(test_timeline());

        let task = update(
            &mut state,
            Message::Timeline(crate::timeline_workspace::Message::ConfirmDiscard),
        );
        assert_eq!(state.phase, Phase::Home);
        assert!(state.timeline.is_none());
        assert!(task.units() == 0);
    }

    #[test]
    fn home_message_ignored_when_not_in_home_phase() {
        let mut state = test_state();
        state.phase = Phase::Timeline;
        state.timeline = Some(test_timeline());

        let task = update(
            &mut state,
            Message::Home(action_guide_home::Message::RecordNew),
        );
        assert_eq!(state.phase, Phase::Timeline);
        assert!(state.timeline.is_some());
        assert!(task.units() == 0);
    }

    #[test]
    fn timeline_message_ignored_when_no_timeline() {
        let mut state = test_state();
        state.phase = Phase::Home;

        let task = update(
            &mut state,
            Message::Timeline(crate::timeline_workspace::Message::SelectStep(1)),
        );
        assert_eq!(state.phase, Phase::Home);
        assert!(task.units() == 0);
    }

    #[test]
    fn project_opened_ignored_when_not_opening() {
        let mut state = test_state();
        state.phase = Phase::Home;

        let ws = test_timeline();
        let task = update(
            &mut state,
            Message::ProjectOpened(ProjectOpenResult::Workspace(std::sync::Arc::new(ws))),
        );
        assert_eq!(state.phase, Phase::Home);
        assert!(state.timeline.is_none());
        assert!(task.units() == 0);
    }

    #[test]
    fn selection_inspected_ignored_when_not_opening() {
        let mut state = test_state();
        state.phase = Phase::Home;

        let task = update(
            &mut state,
            Message::SelectionInspected {
                path: PathBuf::from("/some/project"),
                kind: SelectedDirectoryKind::Project(PathBuf::from("/some/project")),
            },
        );
        assert_eq!(state.phase, Phase::Home);
        assert!(task.units() == 0);
    }

    #[test]
    fn window_ready_is_noop() {
        let mut state = test_state();
        let task = update(&mut state, Message::WindowReady);
        assert_eq!(state.phase, Phase::Home);
        assert!(task.units() == 0);
    }
}
