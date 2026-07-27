use std::path::PathBuf;

use rollshot_action::{VideoImportPass, VideoImportProgress};
use rollshot_agent::jobs::{JobId, LiveJobRegistry};

/// Type alias for the video-import-specific live job registry.
pub type VideoImportJobRegistry =
    LiveJobRegistry<rollshot_action::VideoImportProgress, rollshot_action::ImportedWorkspaceSeed>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImportState {
    Idle,
    Picking,
    ResolvingToolchain,
    SettingUp,
    Preflight,
    AnalyzingPass1,
    ExtractingPass2,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ImportOperationId(u64);

impl ImportOperationId {
    /// Extract the raw nonce for admission construction.
    pub fn get(self) -> u64 {
        self.0
    }
}

/// Error returned when binding a job to a coordinator whose operation has
/// already moved on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImportBindingError {
    StaleOperation,
}

pub struct ImportCoordinator {
    state: ImportState,
    operation_id: Option<ImportOperationId>,
    next_operation_id: u64,
    job_id: Option<JobId>,
    last_progress: Option<VideoImportProgress>,
    pending: Option<(ImportOperationId, PathBuf)>,
}

impl Default for ImportCoordinator {
    fn default() -> Self {
        Self {
            state: ImportState::Idle,
            operation_id: None,
            next_operation_id: 0,
            job_id: None,
            last_progress: None,
            pending: None,
        }
    }
}

impl ImportCoordinator {
    pub fn state(&self) -> ImportState {
        self.state
    }

    pub fn operation_id(&self) -> Option<ImportOperationId> {
        self.operation_id
    }

    pub fn last_progress(&self) -> Option<&VideoImportProgress> {
        self.last_progress.as_ref()
    }

    pub fn job_id(&self) -> Option<&JobId> {
        self.job_id.as_ref()
    }

    pub fn pending_path(&self) -> Option<&PathBuf> {
        self.pending.as_ref().map(|(_, p)| p)
    }

    pub fn set_picking(&mut self) {
        self.state = ImportState::Picking;
    }

    pub fn begin(&mut self, path: PathBuf) -> ImportOperationId {
        let id = ImportOperationId(self.next_operation_id);
        self.next_operation_id += 1;
        self.operation_id = Some(id);
        self.state = ImportState::ResolvingToolchain;
        self.job_id = None;
        self.last_progress = None;
        self.pending = Some((id, path));
        id
    }

    pub fn cancel(&mut self, id: ImportOperationId) {
        if self.operation_id == Some(id) {
            self.state = ImportState::Idle;
            self.operation_id = None;
            self.job_id = None;
            self.last_progress = None;
            self.pending = None;
        }
    }

    pub fn mark_setting_up(&mut self, id: ImportOperationId) {
        if self.operation_id == Some(id) {
            self.state = ImportState::SettingUp;
        }
    }

    pub fn record_progress(&mut self, id: ImportOperationId, progress: VideoImportProgress) {
        if self.operation_id != Some(id) {
            return;
        }
        self.last_progress = Some(progress);
        self.state = match progress.pass {
            VideoImportPass::Preflight => ImportState::Preflight,
            VideoImportPass::Analyze => ImportState::AnalyzingPass1,
            VideoImportPass::Extract => ImportState::ExtractingPass2,
        };
    }

    /// Bind a freshly admitted `JobId` to this coordinator's current operation.
    ///
    /// Rejects the binding if the operation has already moved on.
    pub fn bind_job(
        &mut self,
        operation_id: ImportOperationId,
        job_id: JobId,
    ) -> Result<(), ImportBindingError> {
        if self.operation_id != Some(operation_id) {
            return Err(ImportBindingError::StaleOperation);
        }
        self.job_id = Some(job_id);
        self.state = ImportState::Preflight;
        Ok(())
    }

    /// Project progress from a registry snapshot only if the job matches.
    pub fn project_progress(&mut self, job_id: &JobId, progress: VideoImportProgress) {
        if self.job_id.as_ref() != Some(job_id) {
            return;
        }
        self.last_progress = Some(progress);
        self.state = match progress.pass {
            VideoImportPass::Preflight => ImportState::Preflight,
            VideoImportPass::Analyze => ImportState::AnalyzingPass1,
            VideoImportPass::Extract => ImportState::ExtractingPass2,
        };
    }

    /// Detach the coordinator from its current job, clearing all state.
    pub fn detach(&mut self) {
        self.finish_idle();
    }

    pub fn finish_idle(&mut self) {
        self.state = ImportState::Idle;
        self.operation_id = None;
        self.job_id = None;
        self.last_progress = None;
        self.pending = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn progress(pass: VideoImportPass) -> VideoImportProgress {
        VideoImportProgress {
            pass,
            processed_ms: 0,
            total_ms: 1000,
            retained_candidates: 0,
        }
    }

    #[test]
    fn default_coordinator_is_idle() {
        let coordinator = ImportCoordinator::default();
        assert_eq!(coordinator.state(), ImportState::Idle);
        assert!(coordinator.operation_id().is_none());
    }

    #[test]
    fn begin_transitions_to_resolving() {
        let mut coordinator = ImportCoordinator::default();
        let id = coordinator.begin(PathBuf::from("test.mp4"));
        assert_eq!(coordinator.state(), ImportState::ResolvingToolchain);
        assert_eq!(coordinator.operation_id(), Some(id));
    }

    #[test]
    fn cancelled_or_superseded_operation_ignores_late_messages() {
        let mut coordinator = ImportCoordinator::default();
        let old = coordinator.begin(PathBuf::from("old.mp4"));
        coordinator.cancel(old);
        let new = coordinator.begin(PathBuf::from("new.mp4"));
        coordinator.record_progress(old, progress(VideoImportPass::Extract));
        assert_eq!(coordinator.operation_id(), Some(new));
        assert_ne!(coordinator.state(), ImportState::ExtractingPass2);
    }

    #[test]
    fn cancel_resets_to_idle() {
        let mut coordinator = ImportCoordinator::default();
        let id = coordinator.begin(PathBuf::from("test.mp4"));
        coordinator.cancel(id);
        assert_eq!(coordinator.state(), ImportState::Idle);
        assert!(coordinator.operation_id().is_none());
    }

    #[test]
    fn progress_transitions_through_passes() {
        let mut coordinator = ImportCoordinator::default();
        let id = coordinator.begin(PathBuf::from("test.mp4"));

        coordinator.record_progress(id, progress(VideoImportPass::Preflight));
        assert_eq!(coordinator.state(), ImportState::Preflight);

        coordinator.record_progress(id, progress(VideoImportPass::Analyze));
        assert_eq!(coordinator.state(), ImportState::AnalyzingPass1);

        coordinator.record_progress(id, progress(VideoImportPass::Extract));
        assert_eq!(coordinator.state(), ImportState::ExtractingPass2);
    }

    #[test]
    fn setup_and_worker_start_have_explicit_states() {
        let mut coordinator = ImportCoordinator::default();
        let id = coordinator.begin(PathBuf::from("test.mp4"));

        coordinator.mark_setting_up(id);
        assert_eq!(coordinator.state(), ImportState::SettingUp);
    }

    #[test]
    fn pending_path_survives_across_states() {
        let mut coordinator = ImportCoordinator::default();
        let id = coordinator.begin(PathBuf::from("video.mp4"));
        assert_eq!(
            coordinator.pending_path(),
            Some(&PathBuf::from("video.mp4"))
        );

        coordinator.record_progress(id, progress(VideoImportPass::Preflight));
        assert_eq!(
            coordinator.pending_path(),
            Some(&PathBuf::from("video.mp4"))
        );
    }

    #[test]
    fn finish_idle_clears_everything() {
        let mut coordinator = ImportCoordinator::default();
        let id = coordinator.begin(PathBuf::from("test.mp4"));
        coordinator.record_progress(id, progress(VideoImportPass::Extract));
        coordinator.finish_idle();

        assert_eq!(coordinator.state(), ImportState::Idle);
        assert!(coordinator.operation_id().is_none());
        assert!(coordinator.pending_path().is_none());
    }

    #[test]
    fn coordinator_does_not_retain_source_path_after_completion() {
        let mut coordinator = ImportCoordinator::default();
        let sentinel = "SECRET-user-recording-a1b2.mp4";
        let id = coordinator.begin(PathBuf::from(sentinel));
        coordinator.record_progress(id, progress(VideoImportPass::Preflight));
        coordinator.record_progress(id, progress(VideoImportPass::Analyze));
        coordinator.record_progress(id, progress(VideoImportPass::Extract));
        coordinator.finish_idle();

        assert!(coordinator.pending_path().is_none());
        assert!(coordinator.operation_id().is_none());
        assert!(coordinator.last_progress().is_none());
    }

    #[test]
    fn coordinator_does_not_retain_source_path_after_cancel() {
        let mut coordinator = ImportCoordinator::default();
        let sentinel = "SECRET-user-recording-c3d4.mp4";
        let id = coordinator.begin(PathBuf::from(sentinel));
        coordinator.record_progress(id, progress(VideoImportPass::Analyze));
        coordinator.cancel(id);

        assert!(coordinator.pending_path().is_none());
        assert!(coordinator.operation_id().is_none());
    }

    // ---- Task 4 tests: Job binding and detachment ----

    #[test]
    fn bind_job_keeps_preparation_and_job_identity_distinct() {
        let mut coordinator = ImportCoordinator::default();
        let operation = coordinator.begin(PathBuf::from("test.mp4"));
        let job = JobId::parse("job-00000000-0000-4000-8000-000000000003").unwrap();

        coordinator.bind_job(operation, job.clone()).unwrap();

        assert_eq!(coordinator.operation_id(), Some(operation));
        assert_eq!(coordinator.job_id(), Some(&job));
        assert_eq!(coordinator.state(), ImportState::Preflight);
    }

    #[test]
    fn stale_job_snapshot_cannot_replace_current_progress() {
        let mut coordinator = ImportCoordinator::default();
        let old_operation = coordinator.begin(PathBuf::from("old.mp4"));
        let old_job = JobId::parse("job-00000000-0000-4000-8000-000000000003").unwrap();
        coordinator
            .bind_job(old_operation, old_job.clone())
            .unwrap();
        coordinator.finish_idle();

        let new_operation = coordinator.begin(PathBuf::from("new.mp4"));
        let new_job = JobId::parse("job-00000000-0000-4000-8000-000000000004").unwrap();
        coordinator.bind_job(new_operation, new_job).unwrap();
        coordinator.project_progress(&old_job, progress(VideoImportPass::Extract));

        assert_eq!(coordinator.operation_id(), Some(new_operation));
        assert_ne!(coordinator.state(), ImportState::ExtractingPass2);
    }
}
