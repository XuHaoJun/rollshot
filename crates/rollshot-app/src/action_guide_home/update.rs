use std::path::{Path, PathBuf};

use iced::Task;

use super::recent::RecentProjects;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ActionGuideIntent {
    Home,
    Record { fullscreen: bool },
    Open { path: Option<PathBuf> },
}

impl ActionGuideIntent {
    pub fn capture_request(&self) -> Option<rollshot_capture::CaptureRequest> {
        match self {
            Self::Record { fullscreen: true } => {
                Some(rollshot_capture::CaptureRequest::action_guide_fullscreen())
            }
            Self::Record { fullscreen: false } => {
                Some(rollshot_capture::CaptureRequest::action_guide_region())
            }
            Self::Home | Self::Open { .. } => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SelectedDirectoryKind {
    Project(PathBuf),
    LegacyReader(PathBuf),
    Invalid,
}

pub struct ActionGuideHome {
    pub recent: RecentProjects,
    pub opening: bool,
    pub message: Option<String>,
}

#[derive(Debug, Clone)]
pub enum Message {
    RecordNew,
    OpenPicker,
    PickerSelected(PathBuf),
    PickerCancelled,
    RecentSelected(PathBuf),
    RemoveRecent(PathBuf),
    InspectionResult {
        path: PathBuf,
        kind: SelectedDirectoryKind,
    },
    ProjectOpenResult {
        path: PathBuf,
        result: Result<super::recent::RecentEntry, String>,
    },
    WindowFocused,
    Clear,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    None,
    PickProject,
    InspectSelection(PathBuf),
    RecordNew,
    OpenProject(PathBuf),
    OpenLegacyReader(PathBuf),
}

pub struct Update {
    pub task: Task<Message>,
    pub effect: Effect,
}

impl Update {
    pub fn none() -> Self {
        Self {
            task: Task::none(),
            effect: Effect::None,
        }
    }
}

impl ActionGuideHome {
    pub fn new(recent: RecentProjects) -> Self {
        Self {
            recent,
            opening: false,
            message: None,
        }
    }

    pub fn new_empty() -> Self {
        Self::new(RecentProjects::empty())
    }

    pub fn record_project_open(&mut self, path: PathBuf, display_name: String) {
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis()
            .try_into()
            .unwrap_or(u64::MAX);
        self.record_project_open_at(path, display_name, now_ms);
    }

    fn record_project_open_at(&mut self, path: PathBuf, mut display_name: String, now_ms: u64) {
        if display_name.is_empty() {
            display_name = path
                .file_stem()
                .and_then(|name| name.to_str())
                .unwrap_or("Untitled Guide")
                .to_string();
        }
        self.recent.record_open_at(path, display_name, now_ms);
        self.message = self.recent.save().err();
    }

    pub fn update(&mut self, message: Message) -> Update {
        match message {
            Message::RecordNew => Update {
                task: Task::none(),
                effect: Effect::RecordNew,
            },
            Message::OpenPicker => Update {
                task: Task::none(),
                effect: Effect::PickProject,
            },
            Message::PickerSelected(path) => {
                self.opening = true;
                Update {
                    task: Task::none(),
                    effect: Effect::InspectSelection(path),
                }
            }
            Message::PickerCancelled => Update::none(),
            Message::RecentSelected(path) => {
                if !self.recent_entry_available(&path) {
                    self.recent.remove(&path);
                    if let Err(error) = self.recent.save() {
                        self.message = Some(error);
                    }
                    Update::none()
                } else {
                    self.opening = true;
                    Update {
                        task: Task::none(),
                        effect: Effect::InspectSelection(path),
                    }
                }
            }
            Message::RemoveRecent(path) => {
                self.recent.remove(&path);
                if let Err(error) = self.recent.save() {
                    self.message = Some(error);
                }
                Update::none()
            }
            Message::InspectionResult { path: _, kind } => match kind {
                SelectedDirectoryKind::Project(project_path) => {
                    self.opening = false;
                    self.message = None;
                    Update {
                        task: Task::none(),
                        effect: Effect::OpenProject(project_path),
                    }
                }
                SelectedDirectoryKind::LegacyReader(reader_path) => {
                    self.opening = false;
                    self.message = None;
                    Update {
                        task: Task::none(),
                        effect: Effect::OpenLegacyReader(reader_path),
                    }
                }
                SelectedDirectoryKind::Invalid => {
                    self.opening = false;
                    self.message = Some("Selected path is not a valid project".into());
                    Update::none()
                }
            },
            Message::ProjectOpenResult { path, result } => {
                self.opening = false;
                match result {
                    Ok(entry) => {
                        self.recent.record_open_at(
                            entry.path,
                            entry.display_name,
                            entry.last_opened_ms,
                        );
                        self.message = self.recent.save().err();
                    }
                    Err(err) => {
                        self.message = Some(err);
                    }
                }
                let _ = path;
                Update::none()
            }
            Message::WindowFocused => {
                self.recent.reload();
                Update::none()
            }
            Message::Clear => {
                self.message = None;
                Update::none()
            }
        }
    }

    fn recent_entry_available(&self, path: &Path) -> bool {
        self.recent
            .entries()
            .iter()
            .any(|e| e.path == path && e.available)
    }
}

pub fn subscription() -> iced::Subscription<Message> {
    iced::event::listen_with(|event, _status, _id| match event {
        iced::Event::Window(iced::window::Event::Focused) => Some(Message::WindowFocused),
        _ => None,
    })
}

/// Inspects the shape of a directory to determine its kind.
///
/// Detection rules (exact, no manifest/image reads):
/// - `project.json` present → Project
/// - `session.json` present (without `project.json`) → LegacyReader
/// - otherwise → Invalid
pub fn inspect_selection_shape(path: &Path) -> SelectedDirectoryKind {
    if path.join("project.json").exists() {
        SelectedDirectoryKind::Project(path.to_path_buf())
    } else if path.join("session.json").exists() {
        SelectedDirectoryKind::LegacyReader(path.to_path_buf())
    } else {
        SelectedDirectoryKind::Invalid
    }
}

pub fn legacy_reader_entrypoint(path: &Path) -> Result<PathBuf, &'static str> {
    let index = path.join("index.html");
    if index.is_file() {
        Ok(index)
    } else {
        Err("Legacy Action Guide is missing index.html")
    }
}

/// Blocking inspection with a fake for testing — calls the provided closure
/// instead of hitting the filesystem.
#[cfg(test)]
fn inspect_selection_with(path: &Path, exists_fn: &dyn Fn(&Path) -> bool) -> SelectedDirectoryKind {
    if exists_fn(&path.join("project.json")) {
        SelectedDirectoryKind::Project(path.to_path_buf())
    } else if exists_fn(&path.join("session.json")) {
        SelectedDirectoryKind::LegacyReader(path.to_path_buf())
    } else {
        SelectedDirectoryKind::Invalid
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::action_guide_home::recent::RecentEntry;
    use tempfile::TempDir;

    fn setup_home() -> (TempDir, ActionGuideHome) {
        let dir = TempDir::new().unwrap();
        let config_dir = dir.path().join("rollshot");
        std::fs::create_dir_all(&config_dir).unwrap();
        let recent = RecentProjects::load(&config_dir);
        (dir, ActionGuideHome::new(recent))
    }

    // ---- Record New ----

    #[test]
    fn record_new_emits_record_effect() {
        let (_dir, mut home) = setup_home();
        let update = home.update(Message::RecordNew);
        assert_eq!(update.effect, Effect::RecordNew);
    }

    // ---- Open picker ----

    #[test]
    fn open_picker_emits_pick_project_effect() {
        let (_dir, mut home) = setup_home();
        let update = home.update(Message::OpenPicker);
        assert_eq!(update.effect, Effect::PickProject);
    }

    #[test]
    fn picker_selection_inspects_path_not_in_recents() {
        let (_dir, mut home) = setup_home();
        let path = PathBuf::from("/new/project");

        let update = home.update(Message::PickerSelected(path.clone()));

        assert_eq!(update.effect, Effect::InspectSelection(path));
        assert!(home.opening);
    }

    #[test]
    fn legacy_reader_entrypoint_requires_index_html() {
        let dir = tempfile::tempdir().unwrap();
        assert!(legacy_reader_entrypoint(dir.path()).is_err());

        std::fs::write(dir.path().join("index.html"), "<html></html>").unwrap();
        assert_eq!(
            legacy_reader_entrypoint(dir.path()).unwrap(),
            dir.path().join("index.html")
        );
    }

    #[test]
    fn picker_cancelled_leaves_state_unchanged() {
        let (_dir, mut home) = setup_home();
        let update = home.update(Message::PickerCancelled);
        assert_eq!(update.effect, Effect::None);
        assert!(!home.opening);
        assert!(home.message.is_none());
    }

    #[test]
    fn window_focus_reloads_recent_projects_from_disk() {
        let (dir, mut home) = setup_home();
        let config_dir = dir.path().join("rollshot");
        let project = dir.path().join("saved.rollshot-guide");
        std::fs::create_dir_all(&project).unwrap();
        let mut child_recents = RecentProjects::load(&config_dir);
        child_recents.record_open_at(project.clone(), "Saved Guide".into(), 7);
        child_recents.save().unwrap();

        home.update(Message::WindowFocused);

        assert_eq!(home.recent.entries().len(), 1);
        assert_eq!(home.recent.entries()[0].path, project);
    }

    #[test]
    fn removing_recent_project_persists_removal() {
        let (dir, mut home) = setup_home();
        let config_dir = dir.path().join("rollshot");
        let project = dir.path().join("saved.rollshot-guide");
        home.recent
            .record_open_at(project.clone(), "Saved Guide".into(), 7);
        home.recent.save().unwrap();

        home.update(Message::RemoveRecent(project));

        assert!(RecentProjects::load(&config_dir).entries().is_empty());
    }

    #[test]
    fn recording_project_open_persists_recent_entry() {
        let (dir, mut home) = setup_home();
        let config_dir = dir.path().join("rollshot");
        let project = dir.path().join("saved.rollshot-guide");

        home.record_project_open_at(project.clone(), String::new(), 9);

        let persisted = RecentProjects::load(&config_dir);
        assert_eq!(persisted.entries().len(), 1);
        assert_eq!(persisted.entries()[0].path, project);
        assert_eq!(persisted.entries()[0].display_name, "saved");
    }

    // ---- Recent selection: available entry ----

    #[test]
    fn recent_selected_available_entry_emits_inspect() {
        let dir = TempDir::new().unwrap();
        let config_dir = dir.path().join("rollshot");
        std::fs::create_dir_all(&config_dir).unwrap();
        let mut recent = RecentProjects::load(&config_dir);
        let project_path = dir.path().join("my-project");
        std::fs::create_dir_all(&project_path).unwrap();
        recent.record_open_at(project_path.clone(), "My Project".into(), 1);
        let mut home = ActionGuideHome::new(recent);

        let update = home.update(Message::RecentSelected(project_path.clone()));
        assert_eq!(update.effect, Effect::InspectSelection(project_path));
        assert!(home.opening);
    }

    // ---- Recent selection: unavailable entry ----

    #[test]
    fn recent_selected_unavailable_entry_removes_and_does_nothing() {
        let dir = TempDir::new().unwrap();
        let config_dir = dir.path().join("rollshot");
        std::fs::create_dir_all(&config_dir).unwrap();
        let mut recent = RecentProjects::load(&config_dir);
        let project_path = PathBuf::from("/nonexistent/path");
        recent.record_open_at(project_path.clone(), "Missing".into(), 1);
        recent.refresh_availability();
        let mut home = ActionGuideHome::new(recent);

        assert_eq!(home.recent.entries().len(), 1);
        let update = home.update(Message::RecentSelected(project_path));
        assert_eq!(update.effect, Effect::None);
        assert_eq!(home.recent.entries().len(), 0);
    }

    // ---- Remove recent ----

    #[test]
    fn remove_recent_removes_entry() {
        let dir = TempDir::new().unwrap();
        let config_dir = dir.path().join("rollshot");
        std::fs::create_dir_all(&config_dir).unwrap();
        let mut recent = RecentProjects::load(&config_dir);
        recent.record_open_at(PathBuf::from("/a"), "A".into(), 1);
        recent.record_open_at(PathBuf::from("/b"), "B".into(), 2);
        let mut home = ActionGuideHome::new(recent);

        home.update(Message::RemoveRecent(PathBuf::from("/a")));
        assert_eq!(home.recent.entries().len(), 1);
        assert_eq!(home.recent.entries()[0].path, PathBuf::from("/b"));
    }

    // ---- WindowFocused refreshes availability ----

    #[test]
    fn window_focused_refreshes_availability() {
        let dir = TempDir::new().unwrap();
        let config_dir = dir.path().join("rollshot");
        std::fs::create_dir_all(&config_dir).unwrap();
        let mut recent = RecentProjects::load(&config_dir);
        recent.record_open_at(PathBuf::from("/nonexistent"), "Missing".into(), 1);
        recent.save().unwrap();
        let mut home = ActionGuideHome::new(recent);

        assert!(home.recent.entries()[0].available);
        home.update(Message::WindowFocused);
        assert!(!home.recent.entries()[0].available);
    }

    // ---- Inspection results: project.json ----

    #[test]
    fn inspection_result_project_emits_open_project() {
        let (_dir, mut home) = setup_home();
        home.opening = true;
        let path = PathBuf::from("/some/project");
        let update = home.update(Message::InspectionResult {
            path: path.clone(),
            kind: SelectedDirectoryKind::Project(path.clone()),
        });
        assert_eq!(update.effect, Effect::OpenProject(path));
        assert!(!home.opening);
        assert!(home.message.is_none());
    }

    // ---- Inspection results: legacy session.json ----

    #[test]
    fn inspection_result_legacy_reader_emits_open_legacy_reader() {
        let (_dir, mut home) = setup_home();
        home.opening = true;
        let path = PathBuf::from("/some/legacy");
        let update = home.update(Message::InspectionResult {
            path: path.clone(),
            kind: SelectedDirectoryKind::LegacyReader(path.clone()),
        });
        assert_eq!(update.effect, Effect::OpenLegacyReader(path));
        assert!(!home.opening);
        assert!(home.message.is_none());
    }

    // ---- Inspection results: invalid ----

    #[test]
    fn inspection_result_invalid_sets_message() {
        let (_dir, mut home) = setup_home();
        home.opening = true;
        let update = home.update(Message::InspectionResult {
            path: PathBuf::from("/some/invalid"),
            kind: SelectedDirectoryKind::Invalid,
        });
        assert_eq!(update.effect, Effect::None);
        assert!(!home.opening);
        assert!(home.message.is_some());
    }

    // ---- Background inspection uses blocking fake (no FS in update) ----

    #[test]
    fn inspection_uses_fake_not_filesystem() {
        let fake_path = PathBuf::from("/fake/dir");
        let exists_fn = |p: &Path| -> bool {
            // Verify the paths being checked are the expected ones
            assert!(
                p == Path::new("/fake/dir/project.json")
                    || p == Path::new("/fake/dir/session.json"),
                "unexpected path checked: {}",
                p.display()
            );
            p == Path::new("/fake/dir/project.json")
        };
        let kind = inspect_selection_with(&fake_path, &exists_fn);
        assert_eq!(kind, SelectedDirectoryKind::Project(fake_path));
    }

    #[test]
    fn inspection_legacy_when_only_session_json() {
        let fake_path = PathBuf::from("/fake/dir");
        let exists_fn = |p: &Path| -> bool { p == Path::new("/fake/dir/session.json") };
        let kind = inspect_selection_with(&fake_path, &exists_fn);
        assert_eq!(kind, SelectedDirectoryKind::LegacyReader(fake_path));
    }

    #[test]
    fn inspection_invalid_when_neither_present() {
        let fake_path = PathBuf::from("/fake/dir");
        let exists_fn = |_p: &Path| -> bool { false };
        let kind = inspect_selection_with(&fake_path, &exists_fn);
        assert_eq!(kind, SelectedDirectoryKind::Invalid);
    }

    #[test]
    fn inspection_project_takes_precedence_over_legacy() {
        let fake_path = PathBuf::from("/fake/dir");
        let exists_fn = |p: &Path| -> bool {
            // Both present — project.json wins
            p == Path::new("/fake/dir/project.json") || p == Path::new("/fake/dir/session.json")
        };
        let kind = inspect_selection_with(&fake_path, &exists_fn);
        assert_eq!(kind, SelectedDirectoryKind::Project(fake_path));
    }

    // ---- inspect_selection_shape with real FS ----

    #[test]
    fn inspect_shape_project_json_present() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("project.json"), "{}").unwrap();
        let kind = inspect_selection_shape(dir.path());
        assert_eq!(
            kind,
            SelectedDirectoryKind::Project(dir.path().to_path_buf())
        );
    }

    #[test]
    fn inspect_shape_session_json_without_project_json() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("session.json"), "{}").unwrap();
        let kind = inspect_selection_shape(dir.path());
        assert_eq!(
            kind,
            SelectedDirectoryKind::LegacyReader(dir.path().to_path_buf())
        );
    }

    #[test]
    fn inspect_shape_index_html_reader_handoff() {
        // An index.html without project.json or session.json is Invalid
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("index.html"), "<html></html>").unwrap();
        let kind = inspect_selection_shape(dir.path());
        assert_eq!(kind, SelectedDirectoryKind::Invalid);
    }

    #[test]
    fn inspect_shape_missing_path() {
        let kind = inspect_selection_shape(Path::new("/nonexistent/path"));
        assert_eq!(kind, SelectedDirectoryKind::Invalid);
    }

    // ---- ProjectOpenResult ----

    #[test]
    fn project_open_success_records_in_recent() {
        let dir = TempDir::new().unwrap();
        let config_dir = dir.path().join("rollshot");
        std::fs::create_dir_all(&config_dir).unwrap();
        let recent = RecentProjects::load(&config_dir);
        let mut home = ActionGuideHome::new(recent);

        let project_path = PathBuf::from("/my/project");
        let update = home.update(Message::ProjectOpenResult {
            path: project_path.clone(),
            result: Ok(RecentEntry {
                path: project_path.clone(),
                display_name: "My Project".into(),
                last_opened_ms: 100,
                available: true,
            }),
        });
        assert_eq!(update.effect, Effect::None);
        assert_eq!(home.recent.entries().len(), 1);
        assert_eq!(home.recent.entries()[0].display_name, "My Project");
        assert!(home.message.is_none());
    }

    #[test]
    fn project_open_failure_sets_message() {
        let (_dir, mut home) = setup_home();
        let update = home.update(Message::ProjectOpenResult {
            path: PathBuf::from("/a"),
            result: Err("lock failed".into()),
        });
        assert_eq!(update.effect, Effect::None);
        assert_eq!(home.message.as_deref(), Some("lock failed"));
    }

    // ---- ClearMessage ----

    #[test]
    fn clear_message() {
        let (_dir, mut home) = setup_home();
        home.message = Some("error".into());
        home.update(Message::Clear);
        assert!(home.message.is_none());
    }

    // ---- ActionGuideIntent ----

    #[test]
    fn action_guide_intent_home() {
        let intent = ActionGuideIntent::Home;
        assert_eq!(intent, ActionGuideIntent::Home);
    }

    #[test]
    fn action_guide_intent_record() {
        let intent = ActionGuideIntent::Record { fullscreen: true };
        assert_eq!(intent, ActionGuideIntent::Record { fullscreen: true });
    }

    #[test]
    fn action_guide_intent_open_with_path() {
        let intent = ActionGuideIntent::Open {
            path: Some(PathBuf::from("/test")),
        };
        assert_eq!(
            intent,
            ActionGuideIntent::Open {
                path: Some(PathBuf::from("/test"))
            }
        );
    }

    #[test]
    fn action_guide_intent_open_no_path() {
        let intent = ActionGuideIntent::Open { path: None };
        assert_eq!(intent, ActionGuideIntent::Open { path: None });
    }

    #[test]
    fn record_intent_builds_region_or_fullscreen_request() {
        assert_eq!(
            ActionGuideIntent::Record { fullscreen: false }.capture_request(),
            Some(rollshot_capture::CaptureRequest::action_guide_region())
        );
        assert_eq!(
            ActionGuideIntent::Record { fullscreen: true }.capture_request(),
            Some(rollshot_capture::CaptureRequest::action_guide_fullscreen())
        );
        assert_eq!(ActionGuideIntent::Home.capture_request(), None);
    }
}
