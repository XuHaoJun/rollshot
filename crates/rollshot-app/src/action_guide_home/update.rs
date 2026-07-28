use std::path::{Path, PathBuf};

use iced::Task;

use super::recent::RecentProjects;
use super::video_import::{ImportCoordinator, ImportOperationId, VideoImportJobRegistry};

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
    pub import: ImportCoordinator,
    import_jobs: VideoImportJobRegistry,
    import_job_watch: rollshot_agent::jobs::JobWatch,
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
    ImportRecording,
    ImportPickerSelected(PathBuf),
    ImportPickerCancelled,
    ImportToolchainResolved {
        operation_id: ImportOperationId,
        resolution: crate::managed_ffmpeg::VideoImportToolchainResolution,
    },
    ImportSetupFinished {
        operation_id: ImportOperationId,
        result: Result<(), String>,
    },
    RetryImportSetup,
    ImportJobsChanged,
    CancelImport,
}

pub enum Effect {
    None,
    PickProject,
    InspectSelection(PathBuf),
    RecordNew,
    OpenProject(PathBuf),
    OpenLegacyReader(PathBuf),
    PickRecording,
    StartImport {
        job_id: rollshot_agent::jobs::JobId,
        path: PathBuf,
        toolchain: rollshot_action::VideoToolchain,
        cancellation: rollshot_action::VideoImportCancellation,
        reporter: rollshot_agent::jobs::JobReporter<
            rollshot_action::VideoImportProgress,
            rollshot_action::ImportedWorkspaceSeed,
        >,
    },
    SetupImportToolchain {
        operation_id: ImportOperationId,
    },
    OpenImportedTimeline(rollshot_action::ImportedWorkspaceSeed),
    ResolveImportToolchain {
        operation_id: ImportOperationId,
    },
}

fn truncate_path(p: &std::path::Path) -> String {
    match p.file_name().and_then(|f| f.to_str()) {
        Some(name) => format!("..{SEP}{name}"),
        None => "..".to_string(),
    }
}

const SEP: &str = std::path::MAIN_SEPARATOR_STR;

impl std::fmt::Debug for Effect {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::None => write!(f, "None"),
            Self::PickProject => write!(f, "PickProject"),
            Self::InspectSelection(p) => write!(f, "InspectSelection({})", truncate_path(p)),
            Self::RecordNew => write!(f, "RecordNew"),
            Self::OpenProject(p) => write!(f, "OpenProject({})", truncate_path(p)),
            Self::OpenLegacyReader(p) => write!(f, "OpenLegacyReader({})", truncate_path(p)),
            Self::PickRecording => write!(f, "PickRecording"),
            Self::StartImport { job_id, .. } => f
                .debug_struct("StartImport")
                .field("job_id", job_id)
                .finish_non_exhaustive(),
            Self::SetupImportToolchain { operation_id } => f
                .debug_struct("SetupImportToolchain")
                .field("operation_id", operation_id)
                .finish(),
            Self::ResolveImportToolchain { operation_id } => f
                .debug_struct("ResolveImportToolchain")
                .field("operation_id", operation_id)
                .finish(),
            Self::OpenImportedTimeline(_) => write!(f, "OpenImportedTimeline(..)"),
        }
    }
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
        let import_jobs = VideoImportJobRegistry::new();
        let import_job_watch = import_jobs.watch();
        Self {
            recent,
            opening: false,
            message: None,
            import: ImportCoordinator::default(),
            import_jobs,
            import_job_watch,
        }
    }

    pub fn new_empty() -> Self {
        Self::new(RecentProjects::empty())
    }

    pub fn import_coordinator(&self) -> &ImportCoordinator {
        &self.import
    }

    pub fn import_coordinator_mut(&mut self) -> &mut ImportCoordinator {
        &mut self.import
    }

    /// Access the live job registry (read-only for tests and subscriptions).
    pub fn import_jobs(&self) -> &VideoImportJobRegistry {
        &self.import_jobs
    }

    /// Access the stable job-watch handle for subscription identity.
    pub fn import_job_watch(&self) -> rollshot_agent::jobs::JobWatch {
        self.import_job_watch.clone()
    }

    /// Test-only helper: admit a raw import job and return (job_id, reporter).
    #[cfg(test)]
    pub(crate) fn admit_test_import(
        &self,
        nonce: u64,
    ) -> Result<
        (
            rollshot_agent::jobs::JobId,
            rollshot_agent::jobs::JobReporter<
                rollshot_action::VideoImportProgress,
                rollshot_action::ImportedWorkspaceSeed,
            >,
        ),
        rollshot_agent::jobs::JobAdmissionError,
    > {
        let admission = rollshot_agent::jobs::JobAdmission::action_guide_video_import(nonce);
        let control = rollshot_agent::jobs::JobControl::new(|| {});
        self.import_jobs.admit(admission, control, now_unix_ms())
    }

    /// Test-only helper: admit a job, bind to the coordinator, and return
    /// the (job_id, reporter).
    #[cfg(test)]
    pub(crate) fn bind_test_import(
        &mut self,
    ) -> (
        rollshot_agent::jobs::JobId,
        rollshot_agent::jobs::JobReporter<
            rollshot_action::VideoImportProgress,
            rollshot_action::ImportedWorkspaceSeed,
        >,
    ) {
        let operation = self.import.begin(PathBuf::from("test.mp4"));
        let (job_id, reporter) = self.admit_test_import(operation.get()).unwrap();
        self.import.bind_job(operation, job_id.clone()).unwrap();
        (job_id, reporter)
    }

    /// Test-only helper: admit with a cancellation probe.
    #[cfg(test)]
    pub(crate) fn bind_test_import_with_cancel_probe(
        &mut self,
    ) -> (
        rollshot_agent::jobs::JobId,
        rollshot_agent::jobs::JobReporter<
            rollshot_action::VideoImportProgress,
            rollshot_action::ImportedWorkspaceSeed,
        >,
        std::sync::Arc<std::sync::atomic::AtomicBool>,
    ) {
        let operation = self.import.begin(PathBuf::from("test.mp4"));
        let observed = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let probe = observed.clone();
        let admission =
            rollshot_agent::jobs::JobAdmission::action_guide_video_import(operation.get());
        let control = rollshot_agent::jobs::JobControl::new(move || {
            probe.store(true, std::sync::atomic::Ordering::SeqCst);
        });
        let (job_id, reporter) = self
            .import_jobs
            .admit(admission, control, now_unix_ms())
            .unwrap();
        self.import.bind_job(operation, job_id.clone()).unwrap();
        (job_id, reporter, observed)
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
            Message::ImportRecording => {
                self.import.set_picking();
                Update {
                    task: Task::none(),
                    effect: Effect::PickRecording,
                }
            }
            Message::ImportPickerSelected(path) => {
                let ext = path
                    .extension()
                    .and_then(|e| e.to_str())
                    .unwrap_or("")
                    .to_ascii_lowercase();
                if !matches!(ext.as_str(), "mp4" | "mov" | "mkv" | "webm") {
                    self.import.finish_idle();
                    self.message = Some(format!(
                        "Unsupported video format (.{}). Supported: mp4, mov, mkv, webm",
                        ext
                    ));
                    return Update::none();
                }
                let id = self.import.begin(path);
                Update {
                    task: Task::none(),
                    effect: Effect::ResolveImportToolchain { operation_id: id },
                }
            }
            Message::ImportPickerCancelled => {
                // Silent — coordinator was in Picking, reset to Idle
                self.import.finish_idle();
                Update::none()
            }
            Message::ImportToolchainResolved {
                operation_id,
                resolution,
            } => {
                if self.import.operation_id() != Some(operation_id) {
                    return Update::none();
                }
                match resolution {
                    crate::managed_ffmpeg::VideoImportToolchainResolution::Available(toolchain) => {
                        // Registry admission: create cancellation, admit the job, bind.
                        let cancellation = rollshot_action::VideoImportCancellation::default();
                        let control = cancellation.clone();
                        let admission =
                            rollshot_agent::jobs::JobAdmission::action_guide_video_import(
                                operation_id.get(),
                            );
                        let admitted = self.import_jobs.admit(
                            admission,
                            rollshot_agent::jobs::JobControl::new(move || control.cancel()),
                            now_unix_ms(),
                        );
                        let (job_id, reporter) = match admitted {
                            Ok(admitted) => admitted,
                            Err(error) => {
                                self.import.finish_idle();
                                self.message = Some(import_admission_message(error));
                                return Update::none();
                            }
                        };
                        self.import
                            .bind_job(operation_id, job_id.clone())
                            .expect("fresh admission binds to current operation");

                        let path = self.import.pending_path().cloned().unwrap_or_default();
                        Update {
                            task: Task::none(),
                            effect: Effect::StartImport {
                                job_id,
                                path,
                                toolchain,
                                cancellation,
                                reporter,
                            },
                        }
                    }
                    crate::managed_ffmpeg::VideoImportToolchainResolution::NeedsSetup(_) => {
                        self.import.mark_setting_up(operation_id);
                        Update {
                            task: Task::none(),
                            effect: Effect::SetupImportToolchain { operation_id },
                        }
                    }
                }
            }
            Message::ImportSetupFinished {
                operation_id,
                result,
            } => {
                if self.import.operation_id() != Some(operation_id) {
                    return Update::none();
                }
                match result {
                    Ok(()) => {
                        // Re-resolve after successful setup
                        Update {
                            task: Task::none(),
                            effect: Effect::ResolveImportToolchain { operation_id },
                        }
                    }
                    Err(err) => {
                        self.import.finish_idle();
                        self.message = Some(err);
                        Update::none()
                    }
                }
            }
            Message::RetryImportSetup => {
                // Re-resolve the toolchain after setup
                match self.import.operation_id() {
                    Some(operation_id) => Update {
                        task: Task::none(),
                        effect: Effect::ResolveImportToolchain { operation_id },
                    },
                    None => Update::none(),
                }
            }
            Message::ImportJobsChanged => self.reconcile_import_job(),
            Message::CancelImport => {
                if let Some(job_id) = self.import.job_id().cloned() {
                    let _ = self.import_jobs.cancel(&job_id, now_unix_ms());
                    self.import.detach();
                } else if let Some(id) = self.import.operation_id() {
                    // Pre-job cancellation: no registry call needed
                    self.import.cancel(id);
                }
                Update::none()
            }
        }
    }

    /// Reconcile the current import job against the latest registry snapshot.
    fn reconcile_import_job(&mut self) -> Update {
        let Some(job_id) = self.import.job_id().cloned() else {
            return Update::none();
        };

        let snapshot = match self.import_jobs.snapshot(&job_id) {
            Some(s) => s,
            None => {
                // Record was evicted — detach silently.
                self.import.detach();
                return Update::none();
            }
        };

        match snapshot.state() {
            rollshot_agent::jobs::JobState::Starting
            | rollshot_agent::jobs::JobState::Running
            | rollshot_agent::jobs::JobState::Cancelling => {
                // Project latest progress if available.
                if let Some(progress) = snapshot.progress().cloned() {
                    self.import.project_progress(&job_id, progress);
                }
                Update::none()
            }
            rollshot_agent::jobs::JobState::Succeeded => {
                // Collect once, open the timeline.
                let collect_result = self.import_jobs.collect(&job_id, now_unix_ms());
                match collect_result {
                    Ok(seed) => {
                        self.import.finish_idle();
                        Update {
                            task: Task::none(),
                            effect: Effect::OpenImportedTimeline(seed),
                        }
                    }
                    Err(_) => {
                        // Already collected or expired — detach.
                        self.import.finish_idle();
                        Update::none()
                    }
                }
            }
            rollshot_agent::jobs::JobState::Failed => {
                let msg = snapshot
                    .failure_category()
                    .map(job_failure_message)
                    .unwrap_or("Import worker stopped unexpectedly.")
                    .to_string();
                self.import.finish_idle();
                self.message = Some(msg);
                Update::none()
            }
            rollshot_agent::jobs::JobState::Cancelled => {
                self.import.finish_idle();
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

fn now_unix_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}

fn video_import_failure_category(
    error: &rollshot_action::VideoImportError,
) -> Option<rollshot_agent::jobs::JobFailureCategory> {
    use rollshot_action::VideoImportError as Error;
    use rollshot_agent::jobs::JobFailureCategory as Category;
    match error {
        Error::ProbeFailed => Some(Category::ProbeFailed),
        Error::MissingVideoStream => Some(Category::MissingVideoStream),
        Error::InvalidVideoMetadata => Some(Category::InvalidVideoMetadata),
        Error::DecoderUnavailable => Some(Category::DecoderUnavailable),
        Error::DecodeFailed => Some(Category::DecodeFailed),
        Error::EvidenceMissing => Some(Category::EvidenceMissing),
        Error::ScratchIo => Some(Category::ScratchIo),
        Error::ResourceLimit => Some(Category::ResourceLimit),
        Error::Cancelled => None,
    }
}

fn video_import_error_message(error: &rollshot_action::VideoImportError) -> String {
    match error {
        rollshot_action::VideoImportError::Cancelled => error.to_string(),
        _ => format!("Import failed: {error}"),
    }
}

fn job_failure_message(category: rollshot_agent::jobs::JobFailureCategory) -> &'static str {
    use rollshot_agent::jobs::JobFailureCategory as Category;
    match category {
        Category::ProbeFailed => "Import failed: Video metadata could not be read.",
        Category::MissingVideoStream => {
            "Import failed: The selected file has no readable video stream."
        }
        Category::InvalidVideoMetadata => {
            "Import failed: The selected video has invalid dimensions or duration."
        }
        Category::DecoderUnavailable => "Import failed: The video decoder is unavailable.",
        Category::DecodeFailed => "Import failed: The video could not be decoded.",
        Category::EvidenceMissing => "Import failed: Required evidence could not be extracted.",
        Category::ScratchIo => "Import failed: Temporary evidence storage failed.",
        Category::ResourceLimit => {
            "Import failed: The recording exceeds an internal resource bound."
        }
        Category::WorkerAbandoned | Category::WorkerPanic => "Import worker stopped unexpectedly.",
    }
}

fn import_admission_message(error: rollshot_agent::jobs::JobAdmissionError) -> String {
    match error {
        rollshot_agent::jobs::JobAdmissionError::ActiveLimit { .. }
        | rollshot_agent::jobs::JobAdmissionError::ResultCapacity { .. }
        | rollshot_agent::jobs::JobAdmissionError::TerminalCapacity { .. } => {
            "Too many imports are still active or awaiting cleanup.".to_string()
        }
        rollshot_agent::jobs::JobAdmissionError::ShuttingDown => {
            "Cannot start import while the application is shutting down.".to_string()
        }
        rollshot_agent::jobs::JobAdmissionError::KindAuthorityMismatch
        | rollshot_agent::jobs::JobAdmissionError::OwnerAuthorityMismatch
        | rollshot_agent::jobs::JobAdmissionError::UnsupportedAuthoritySource => {
            "Import could not start because authorization was rejected.".to_string()
        }
    }
}

/// Build an iced stream that emits `ImportJobsChanged` on every watch revision.
fn import_job_changes(
    watch: &rollshot_agent::jobs::JobWatch,
) -> impl iced::futures::Stream<Item = Message> {
    use iced::futures::SinkExt;

    // Clone the receiver from the watch handle. run_with passes &D, so we
    // must clone the receiver here rather than calling receiver() on a shared ref.
    let mut watch_mut = watch.clone();
    let mut receiver = watch_mut.receiver();
    iced::stream::channel(1, async move |mut output| loop {
        if receiver.changed().await.is_err() {
            return;
        }
        if output.send(Message::ImportJobsChanged).await.is_err() {
            return;
        }
    })
}

pub fn subscription(home: &ActionGuideHome) -> iced::Subscription<Message> {
    let watch = home.import_job_watch();
    let jobs = iced::Subscription::run_with(watch, import_job_changes);
    iced::Subscription::batch([
        iced::event::listen_with(|event, _status, _id| match event {
            iced::Event::Window(iced::window::Event::Focused) => Some(Message::WindowFocused),
            _ => None,
        }),
        jobs,
    ])
}

pub(crate) fn run_import_task(
    path: PathBuf,
    toolchain: rollshot_action::VideoToolchain,
    cancellation: rollshot_action::VideoImportCancellation,
    reporter: rollshot_agent::jobs::JobReporter<
        rollshot_action::VideoImportProgress,
        rollshot_action::ImportedWorkspaceSeed,
    >,
) -> Task<Message> {
    // Wrap reporter in Mutex for interior mutability inside the Fn progress
    // callback required by import_video. Mutex is UnwindSafe.
    let reporter = std::sync::Mutex::new(reporter);
    Task::perform(
        async move {
            let worker = tokio::task::spawn_blocking(move || {
                {
                    let mut r = reporter.lock().unwrap();
                    if r.mark_running(now_unix_ms()).is_err() {
                        return;
                    }
                }
                let request = rollshot_action::VideoImportRequest {
                    input: path,
                    toolchain,
                    scratch_parent: std::env::temp_dir().join("rollshot/import"),
                };
                let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    rollshot_action::import_video(request, cancellation, |progress| {
                        let _ = reporter
                            .lock()
                            .unwrap()
                            .report_progress(progress, now_unix_ms());
                    })
                }));
                let mut r = reporter.lock().unwrap();
                match outcome {
                    Ok(Ok(seed)) => {
                        let _ = r.succeed(seed, now_unix_ms());
                    }
                    Ok(Err(rollshot_action::VideoImportError::Cancelled)) => {
                        let _ = r.cancelled(now_unix_ms());
                    }
                    Ok(Err(error)) => {
                        let category = video_import_failure_category(&error)
                            .expect("cancelled handled by the previous arm");
                        let _ = r.fail(category, now_unix_ms());
                    }
                    Err(_) => {
                        let _ = r.fail(
                            rollshot_agent::jobs::JobFailureCategory::WorkerPanic,
                            now_unix_ms(),
                        );
                    }
                }
            })
            .await;

            if worker.is_err() {
                tracing::event!(
                    target: "rollshot::app::action_guide::video_import",
                    tracing::Level::WARN,
                    category = "worker_join_failed",
                );
            }
            Message::ImportJobsChanged
        },
        std::convert::identity,
    )
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
    use crate::action_guide_home::video_import::ImportState;
    use rollshot_action::VideoImportPass;
    use rollshot_agent::jobs::{JobFailureCategory, JobState, TERMINAL_TTL_MS};
    use std::sync::atomic::Ordering;
    use tempfile::TempDir;

    fn setup_home() -> (TempDir, ActionGuideHome) {
        let dir = TempDir::new().unwrap();
        let config_dir = dir.path().join("rollshot");
        std::fs::create_dir_all(&config_dir).unwrap();
        let recent = RecentProjects::load(&config_dir);
        (dir, ActionGuideHome::new(recent))
    }

    fn toolchain_fixture() -> rollshot_action::VideoToolchain {
        rollshot_action::VideoToolchain {
            ffprobe: std::path::PathBuf::from("/usr/bin/ffprobe"),
            ffmpeg: std::path::PathBuf::from("/usr/bin/ffmpeg"),
        }
    }

    fn dummy_seed(scratch_dir: &tempfile::TempDir) -> rollshot_action::ImportedWorkspaceSeed {
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

    fn progress(pass: rollshot_action::VideoImportPass) -> rollshot_action::VideoImportProgress {
        rollshot_action::VideoImportProgress {
            pass,
            processed_ms: 0,
            total_ms: 1000,
            retained_candidates: 0,
        }
    }

    // ---- Record New ----

    #[test]
    fn record_new_emits_record_effect() {
        let (_dir, mut home) = setup_home();
        let update = home.update(Message::RecordNew);
        assert!(matches!(update.effect, Effect::RecordNew));
    }

    // ---- Open picker ----

    #[test]
    fn open_picker_emits_pick_project_effect() {
        let (_dir, mut home) = setup_home();
        let update = home.update(Message::OpenPicker);
        assert!(matches!(update.effect, Effect::PickProject));
    }

    #[test]
    fn picker_selection_inspects_path_not_in_recents() {
        let (_dir, mut home) = setup_home();
        let path = PathBuf::from("/new/project");

        let update = home.update(Message::PickerSelected(path.clone()));

        assert!(matches!(update.effect, Effect::InspectSelection(ref p) if p == &path));
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
        assert!(matches!(update.effect, Effect::None));
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
        assert!(matches!(update.effect, Effect::InspectSelection(ref p) if p == &project_path));
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
        assert!(matches!(update.effect, Effect::None));
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
        assert!(matches!(update.effect, Effect::OpenProject(ref p) if p == &path));
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
        assert!(matches!(update.effect, Effect::OpenLegacyReader(ref p) if p == &path));
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
        assert!(matches!(update.effect, Effect::None));
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
        assert!(matches!(update.effect, Effect::None));
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
        assert!(matches!(update.effect, Effect::None));
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

    // ---- Import flow ----

    #[test]
    fn import_recording_emits_pick_recording_effect() {
        let (_dir, mut home) = setup_home();
        let update = home.update(Message::ImportRecording);
        assert!(matches!(update.effect, Effect::PickRecording));
    }

    #[test]
    fn picker_cancel_is_silent() {
        let (_dir, mut home) = setup_home();
        home.update(Message::ImportRecording);
        home.update(Message::ImportPickerCancelled);
        assert_eq!(home.import_coordinator().state(), ImportState::Idle);
        assert!(home.message.is_none());
    }

    #[test]
    fn picker_selected_emits_resolve_toolchain_effect() {
        let (_dir, mut home) = setup_home();
        let update = home.update(Message::ImportPickerSelected(PathBuf::from("video.mp4")));
        assert!(matches!(
            update.effect,
            Effect::ResolveImportToolchain { .. }
        ));
        assert_eq!(
            home.import_coordinator().state(),
            ImportState::ResolvingToolchain
        );
    }

    #[test]
    fn picker_selected_unsupported_extension_sets_message() {
        let (_dir, mut home) = setup_home();
        let update = home.update(Message::ImportPickerSelected(PathBuf::from("clip.avi")));
        assert!(matches!(update.effect, Effect::None));
        assert_eq!(home.import_coordinator().state(), ImportState::Idle);
        assert!(home
            .message
            .as_deref()
            .unwrap()
            .contains("Unsupported video format"));
        assert!(home.message.as_deref().unwrap().contains(".avi"));
    }

    #[test]
    fn cancelled_or_superseded_operation_ignores_late_messages() {
        let mut coordinator = ImportCoordinator::default();
        let old = coordinator.begin(PathBuf::from("old.mp4"));
        coordinator.cancel(old);
        let new = coordinator.begin(PathBuf::from("new.mp4"));
        coordinator.record_progress(
            old,
            rollshot_action::VideoImportProgress {
                pass: rollshot_action::VideoImportPass::Extract,
                processed_ms: 0,
                total_ms: 1000,
                retained_candidates: 0,
            },
        );
        assert_eq!(coordinator.operation_id(), Some(new));
        assert_ne!(coordinator.state(), ImportState::ExtractingPass2);
    }

    // ---- Task 4 tests: registry-backed home update ----

    #[test]
    fn available_toolchain_admits_once_before_worker_effect() {
        let (_dir, mut home) = setup_home();
        let operation = home
            .import_coordinator_mut()
            .begin(PathBuf::from("test.mp4"));
        let update = home.update(Message::ImportToolchainResolved {
            operation_id: operation,
            resolution: crate::managed_ffmpeg::VideoImportToolchainResolution::Available(
                toolchain_fixture(),
            ),
        });

        let Effect::StartImport { job_id, .. } = update.effect else {
            panic!("expected registry-backed StartImport");
        };
        assert_eq!(home.import_coordinator().job_id(), Some(&job_id));
        assert_eq!(home.import_jobs().list().len(), 1);
        assert_eq!(
            home.import_jobs().snapshot(&job_id).unwrap().state(),
            rollshot_agent::jobs::JobState::Starting
        );
    }

    #[test]
    fn registry_admission_failure_starts_no_worker() {
        let (_dir, mut home) = setup_home();
        let mut held_reporters = Vec::new();
        for nonce in 100..104 {
            held_reporters.push(home.admit_test_import(nonce).unwrap().1);
        }
        let operation = home
            .import_coordinator_mut()
            .begin(PathBuf::from("test.mp4"));
        let update = home.update(Message::ImportToolchainResolved {
            operation_id: operation,
            resolution: crate::managed_ffmpeg::VideoImportToolchainResolution::Available(
                toolchain_fixture(),
            ),
        });

        assert!(matches!(update.effect, Effect::None));
        assert!(home
            .message
            .as_deref()
            .unwrap()
            .contains("Too many imports"));
        assert!(home.import_coordinator().job_id().is_none());
        drop(held_reporters);
    }

    #[test]
    fn terminal_snapshot_opens_seed_once_even_after_notification_coalescing() {
        let (_dir, mut home) = setup_home();
        let (job_id, mut reporter) = home.bind_test_import();
        let t = now_unix_ms();
        reporter.mark_running(t).unwrap();
        reporter
            .report_progress(progress(rollshot_action::VideoImportPass::Analyze), t + 1)
            .unwrap();
        reporter
            .succeed(dummy_seed(&tempfile::tempdir().unwrap()), t + 2)
            .unwrap();

        let first = home.update(Message::ImportJobsChanged);
        assert!(matches!(first.effect, Effect::OpenImportedTimeline(_)));
        let second = home.update(Message::ImportJobsChanged);
        assert!(matches!(second.effect, Effect::None));
        assert!(matches!(
            home.import_jobs().collect(&job_id, t + 3),
            Err(rollshot_agent::jobs::JobCollectError::AlreadyCollected)
        ));
    }

    #[test]
    fn cancel_detaches_ui_but_registry_waits_for_worker_confirmation() {
        let (_dir, mut home) = setup_home();
        let (job_id, mut reporter, observed_cancel) = home.bind_test_import_with_cancel_probe();
        let t = now_unix_ms();
        reporter.mark_running(t).unwrap();

        home.update(Message::CancelImport);

        assert!(observed_cancel.load(Ordering::SeqCst));
        assert_eq!(home.import_coordinator().state(), ImportState::Idle);
        assert_eq!(
            home.import_jobs().snapshot(&job_id).unwrap().state(),
            rollshot_agent::jobs::JobState::Cancelling
        );
        reporter.cancelled(t + 1).unwrap();
        assert_eq!(
            home.import_jobs().snapshot(&job_id).unwrap().state(),
            rollshot_agent::jobs::JobState::Cancelled
        );
    }

    #[test]
    fn video_import_errors_map_to_stable_categories_and_existing_copy() {
        use rollshot_action::VideoImportError as Error;
        use rollshot_agent::jobs::JobFailureCategory as Category;

        let cases = [
            (
                Error::ProbeFailed,
                Some(Category::ProbeFailed),
                "Import failed: Video metadata could not be read.",
            ),
            (
                Error::MissingVideoStream,
                Some(Category::MissingVideoStream),
                "Import failed: The selected file has no readable video stream.",
            ),
            (
                Error::InvalidVideoMetadata,
                Some(Category::InvalidVideoMetadata),
                "Import failed: The selected video has invalid dimensions or duration.",
            ),
            (
                Error::DecoderUnavailable,
                Some(Category::DecoderUnavailable),
                "Import failed: The video decoder is unavailable.",
            ),
            (
                Error::DecodeFailed,
                Some(Category::DecodeFailed),
                "Import failed: The video could not be decoded.",
            ),
            (
                Error::EvidenceMissing,
                Some(Category::EvidenceMissing),
                "Import failed: Required evidence could not be extracted.",
            ),
            (
                Error::ScratchIo,
                Some(Category::ScratchIo),
                "Import failed: Temporary evidence storage failed.",
            ),
            (
                Error::ResourceLimit,
                Some(Category::ResourceLimit),
                "Import failed: The recording exceeds an internal resource bound.",
            ),
            (Error::Cancelled, None, "Import was cancelled."),
        ];

        for (error, category, message) in cases {
            assert_eq!(video_import_failure_category(&error), category);
            assert_eq!(video_import_error_message(&error), message);
        }
        assert_eq!(
            job_failure_message(Category::WorkerAbandoned),
            "Import worker stopped unexpectedly."
        );
        assert_eq!(
            job_failure_message(Category::WorkerPanic),
            "Import worker stopped unexpectedly."
        );
    }

    // ---- Task 5: adversarial failure-injection tests ----

    fn seed_with_root(
        parent: &tempfile::TempDir,
    ) -> (rollshot_action::ImportedWorkspaceSeed, std::path::PathBuf) {
        let seed = dummy_seed(parent);
        let root = seed.scratch.root().to_path_buf();
        (seed, root)
    }

    #[test]
    fn notification_loss_does_not_lose_terminal_or_duplicate_collection() {
        let (_project_dir, mut home) = setup_home();
        let scratch_parent = tempfile::tempdir().unwrap();
        let (job_id, mut reporter) = home.bind_test_import();
        let t = now_unix_ms();
        reporter.mark_running(t).unwrap();
        reporter
            .report_progress(progress(VideoImportPass::Analyze), t + 1)
            .unwrap();
        reporter
            .report_progress(progress(VideoImportPass::Extract), t + 2)
            .unwrap();
        reporter
            .succeed(dummy_seed(&scratch_parent), t + 3)
            .unwrap();

        assert_eq!(
            home.import_jobs()
                .snapshot(&job_id)
                .unwrap()
                .progress()
                .unwrap()
                .pass,
            VideoImportPass::Extract
        );
        let first = home.update(Message::ImportJobsChanged);
        assert!(matches!(first.effect, Effect::OpenImportedTimeline(_)));
        let second = home.update(Message::ImportJobsChanged);
        assert!(matches!(second.effect, Effect::None));
        assert_eq!(
            home.import_jobs().collect(&job_id, t + 4).unwrap_err(),
            rollshot_agent::jobs::JobCollectError::AlreadyCollected
        );
    }

    #[test]
    fn cancel_wins_against_late_success_and_drops_seed() {
        let (_project_dir, mut home) = setup_home();
        let scratch_parent = tempfile::tempdir().unwrap();
        let (seed, scratch_root) = seed_with_root(&scratch_parent);
        let (job_id, mut reporter, observed_cancel) = home.bind_test_import_with_cancel_probe();
        reporter.mark_running(10).unwrap();

        home.update(Message::CancelImport);
        reporter.succeed(seed, 11).unwrap();
        let update = home.update(Message::ImportJobsChanged);

        assert!(observed_cancel.load(Ordering::SeqCst));
        assert!(!scratch_root.exists());
        assert!(matches!(update.effect, Effect::None));
        assert_eq!(
            home.import_jobs().snapshot(&job_id).unwrap().state(),
            JobState::Cancelled
        );
    }

    #[test]
    fn stale_terminal_from_old_job_cannot_open_over_new_import() {
        let (_project_dir, mut home) = setup_home();
        let (old_id, mut old_reporter) = home.bind_test_import();
        old_reporter.mark_running(10).unwrap();
        home.import_coordinator_mut().detach();
        let (new_id, mut new_reporter) = home.bind_test_import();
        new_reporter.mark_running(11).unwrap();

        old_reporter
            .fail(JobFailureCategory::DecodeFailed, 12)
            .unwrap();
        let update = home.update(Message::ImportJobsChanged);

        assert!(matches!(update.effect, Effect::None));
        assert_eq!(home.import_coordinator().job_id(), Some(&new_id));
        assert_eq!(
            home.import_jobs().snapshot(&old_id).unwrap().state(),
            JobState::Failed
        );
        assert_eq!(
            home.import_jobs().snapshot(&new_id).unwrap().state(),
            JobState::Running
        );
    }

    #[test]
    fn reporter_drop_becomes_worker_abandoned_and_is_repairable() {
        let (_project_dir, mut home) = setup_home();
        let (job_id, mut reporter) = home.bind_test_import();
        reporter.mark_running(10).unwrap();
        drop(reporter);

        let update = home.update(Message::ImportJobsChanged);

        assert!(matches!(update.effect, Effect::None));
        assert_eq!(home.import_coordinator().state(), ImportState::Idle);
        assert_eq!(
            home.import_jobs()
                .snapshot(&job_id)
                .unwrap()
                .failure_category(),
            Some(JobFailureCategory::WorkerAbandoned)
        );
        assert_eq!(
            home.message.as_deref(),
            Some("Import worker stopped unexpectedly.")
        );
    }

    #[test]
    fn expired_uncollected_seed_is_dropped_and_scratch_is_removed() {
        let (_project_dir, mut home) = setup_home();
        let scratch_parent = tempfile::tempdir().unwrap();
        let (seed, scratch_root) = seed_with_root(&scratch_parent);
        let (job_id, mut reporter) = home.bind_test_import();
        reporter.mark_running(10).unwrap();
        reporter.succeed(seed, 11).unwrap();

        home.import_jobs().prune(11 + TERMINAL_TTL_MS);

        assert!(!scratch_root.exists());
        assert_eq!(
            home.import_jobs()
                .collect(&job_id, 11 + TERMINAL_TTL_MS)
                .unwrap_err(),
            rollshot_agent::jobs::JobCollectError::ResultExpired
        );
        assert!(!format!("{:?}", home.import_jobs().watch())
            .contains(scratch_root.to_string_lossy().as_ref()));
    }
}
