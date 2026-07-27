//! Process-local live job identity, admission, and active-state contracts.
//!
//! V1 limits:
//! - `MAX_ACTIVE_JOBS` = 4
//! - `MAX_UNCOLLECTED_RESULT_SLOTS` = 4
//! - `MAX_TERMINAL_JOBS` = 128
//! - `TERMINAL_TTL_MS` = 5 minutes
//! - `MAX_DIAGNOSTIC_ENTRIES` = 64
//! - `MAX_DIAGNOSTIC_BYTES` = 256 bytes per entry
//!
//! `JobKind` V1 contains only `ActionGuideVideoImport`.
//! `JobExecutionClass` V1 contains only `LocalWorkerWithChildProcesses`.

use std::collections::HashMap;
use std::fmt;
use std::sync::{Arc, Mutex};

use tokio::sync::watch;
use uuid::Uuid;

use crate::authority::AuthoritySnapshot;
use crate::domain::RunId;
use crate::product_task::{ProductTaskId, TaskAttemptId};

// ========================================================================
// V1 limits
// ========================================================================

pub const MAX_ACTIVE_JOBS: usize = 4;
pub const MAX_UNCOLLECTED_RESULT_SLOTS: usize = 4;
pub const MAX_TERMINAL_JOBS: usize = 128;
pub const TERMINAL_TTL_MS: u64 = 5 * 60 * 1000;
pub const MAX_DIAGNOSTIC_ENTRIES: usize = 64;
pub const MAX_DIAGNOSTIC_BYTES: usize = 256;

// ========================================================================
// Identity
// ========================================================================

/// Opaque `job-<UUID>` identifier, generated at successful admission.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct JobId(String);

impl JobId {
    fn new() -> Self {
        Self(format!("job-{}", Uuid::new_v4()))
    }

    /// Parse an existing job ID string (validates `job-` prefix and UUID).
    pub fn parse(value: impl Into<String>) -> Result<Self, String> {
        let value = value.into();
        if !value.starts_with("job-") {
            return Err(format!("job ID must start with 'job-': {value}"));
        }
        let uuid_str = &value[4..];
        Uuid::parse_str(uuid_str).map_err(|e| format!("invalid UUID in job ID: {e}"))?;
        Ok(Self(value))
    }

    /// Borrow the string form.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

// ========================================================================
// Kind and execution class (closed V1 enums)
// ========================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum JobKind {
    ActionGuideVideoImport,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum JobExecutionClass {
    LocalWorkerWithChildProcesses,
}

// ========================================================================
// Product surface
// ========================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ProductSurface {
    ActionGuideHome,
}

// ========================================================================
// Task reference
// ========================================================================

/// Exact Product Task correlation. Metadata, not permission.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct JobTaskRef {
    task_id: ProductTaskId,
    attempt_id: TaskAttemptId,
    run_id: RunId,
}

impl JobTaskRef {
    pub fn new(task_id: ProductTaskId, attempt_id: TaskAttemptId, run_id: RunId) -> Self {
        Self {
            task_id,
            attempt_id,
            run_id,
        }
    }
}

// ========================================================================
// Owner
// ========================================================================

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum JobOwner {
    DirectProductAction {
        surface: ProductSurface,
        operation_nonce: u64,
    },
    ProductTask(JobTaskRef),
}

// ========================================================================
// Direct user action (closed V1)
// ========================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DirectUserAction {
    ActionGuideVideoImport,
}

// ========================================================================
// Authority source
// ========================================================================

/// The authority source that backed a job admission.
///
/// `AgentTask` is represented but rejected as `UnsupportedAuthoritySource` in V1.
pub enum JobAuthoritySource {
    DirectUserAction(DirectUserAction),
    AgentTask {
        authority_snapshot: AuthoritySnapshot,
        task: JobTaskRef,
    },
}

// ========================================================================
// Admission (checked value)
// ========================================================================

/// Checked admission value: kind, owner, execution class, and authority source.
pub struct JobAdmission {
    kind: JobKind,
    execution_class: JobExecutionClass,
    owner: JobOwner,
    authority: JobAuthoritySource,
}

impl JobAdmission {
    /// Construct a direct Action Guide video import admission.
    pub fn action_guide_video_import(operation_nonce: u64) -> Self {
        Self {
            kind: JobKind::ActionGuideVideoImport,
            execution_class: JobExecutionClass::LocalWorkerWithChildProcesses,
            owner: JobOwner::DirectProductAction {
                surface: ProductSurface::ActionGuideHome,
                operation_nonce,
            },
            authority: JobAuthoritySource::DirectUserAction(
                DirectUserAction::ActionGuideVideoImport,
            ),
        }
    }

    /// Construct an agent-task admission (represented but rejected in V1).
    pub fn agent_task(
        kind: JobKind,
        execution_class: JobExecutionClass,
        authority_snapshot: AuthoritySnapshot,
        task: JobTaskRef,
    ) -> Self {
        Self {
            kind,
            execution_class,
            owner: JobOwner::ProductTask(task.clone()),
            authority: JobAuthoritySource::AgentTask {
                authority_snapshot,
                task,
            },
        }
    }

    /// Test-only constructor for arbitrary authority/owner combinations.
    #[cfg(test)]
    pub(crate) fn for_test(
        kind: JobKind,
        execution_class: JobExecutionClass,
        owner: JobOwner,
        authority: JobAuthoritySource,
    ) -> Self {
        Self {
            kind,
            execution_class,
            owner,
            authority,
        }
    }
}

// ========================================================================
// JobControl (cancellation callback)
// ========================================================================

/// Cancellation callback. Stores the user-supplied cancellation closure.
pub struct JobControl {
    cancel_fn: Arc<dyn Fn() + Send + Sync>,
}

impl JobControl {
    pub fn new(cancel_fn: impl Fn() + Send + Sync + 'static) -> Self {
        Self {
            cancel_fn: Arc::new(cancel_fn),
        }
    }

    fn invoke(&self) {
        (self.cancel_fn)();
    }
}

impl fmt::Debug for JobControl {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("JobControl(<redacted>)")
    }
}

// ========================================================================
// Job state
// ========================================================================

/// Closed lifecycle state.
///
/// ```text
/// Starting → Running → Succeeded
///                    → Failed
///                    → Cancelling → Cancelled
/// Starting ─────────→ Cancelling → Cancelled
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum JobState {
    Starting,
    Running,
    Cancelling,
    Succeeded,
    Failed,
    Cancelled,
}

impl JobState {
    fn is_terminal(self) -> bool {
        matches!(
            self,
            JobState::Succeeded | JobState::Failed | JobState::Cancelled
        )
    }

    fn is_active(self) -> bool {
        matches!(
            self,
            JobState::Starting | JobState::Running | JobState::Cancelling
        )
    }
}

// ========================================================================
// Errors
// ========================================================================

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum JobAdmissionError {
    #[error("job kind and authority source do not match")]
    KindAuthorityMismatch,
    #[error("job owner and authority source do not match")]
    OwnerAuthorityMismatch,
    #[error("job authority source is unsupported")]
    UnsupportedAuthoritySource,
    #[error("job registry is shutting down")]
    ShuttingDown,
    #[error("active job limit reached: {limit}")]
    ActiveLimit { limit: usize },
    #[error("terminal job capacity reached: {limit}")]
    TerminalCapacity { limit: usize },
    #[error("active and uncollected result slots reached: {limit}")]
    ResultCapacity { limit: usize },
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum JobTransitionError {
    #[error("job not found")]
    NotFound,
    #[error("job reporter is stale")]
    StaleReporter,
    #[error("invalid transition from {from:?} via {operation}")]
    InvalidTransition {
        from: JobState,
        operation: &'static str,
    },
    #[error("conflicting terminal report")]
    TerminalConflict,
}

// ========================================================================
// Snapshot (observation without R or callback)
// ========================================================================

/// Observation of a single job's current state. Does not contain `R`,
/// the cancellation callback, path, PID, child handle, or raw log.
pub struct JobSnapshot<P> {
    id: JobId,
    kind: JobKind,
    execution_class: JobExecutionClass,
    owner: JobOwner,
    state: JobState,
    progress: Option<P>,
    revision: u64,
    created_at_ms: u64,
}

impl<P> JobSnapshot<P> {
    pub fn id(&self) -> &JobId {
        &self.id
    }

    pub fn kind(&self) -> JobKind {
        self.kind
    }

    pub fn execution_class(&self) -> JobExecutionClass {
        self.execution_class
    }

    pub fn owner(&self) -> &JobOwner {
        &self.owner
    }

    pub fn state(&self) -> JobState {
        self.state
    }

    pub fn progress(&self) -> Option<&P> {
        self.progress.as_ref()
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn created_at_ms(&self) -> u64 {
        self.created_at_ms
    }
}

// ========================================================================
// Internal job record
// ========================================================================

struct JobRecord<P> {
    kind: JobKind,
    execution_class: JobExecutionClass,
    owner: JobOwner,
    state: JobState,
    control: JobControl,
    progress: Option<P>,
    revision: u64,
    created_at_ms: u64,
}

// ========================================================================
// Registry inner state
// ========================================================================

struct RegistryState<P> {
    jobs: HashMap<JobId, JobRecord<P>>,
    shutting_down: bool,
    watch_revision: u64,
}

// ========================================================================
// Registry inner (shared behind Arc)
// ========================================================================

struct Inner<P, R> {
    state: Mutex<RegistryState<P>>,
    watch_tx: watch::Sender<u64>,
    _result_marker: std::marker::PhantomData<R>,
}

// ========================================================================
// LiveJobRegistry
// ========================================================================

/// Process-local live job registry. Generic over structured progress `P`
/// and successful result `R`.
pub struct LiveJobRegistry<P, R> {
    inner: Arc<Inner<P, R>>,
}

impl<P, R> LiveJobRegistry<P, R> {
    /// Create a new empty registry.
    pub fn new() -> Self {
        let (watch_tx, _) = watch::channel(0);
        Self {
            inner: Arc::new(Inner {
                state: Mutex::new(RegistryState {
                    jobs: HashMap::new(),
                    shutting_down: false,
                    watch_revision: 0,
                }),
                watch_tx,
                _result_marker: std::marker::PhantomData,
            }),
        }
    }

    /// Admit a new job. Validates in order: registry open, authority/owner/kind
    /// match, prune eligible terminal entries, terminal capacity, active capacity,
    /// reserved result-slot capacity, then allocate/insert.
    ///
    /// Returns `(JobId, JobReporter<P, R>)` on success.
    pub fn admit(
        &self,
        admission: JobAdmission,
        control: JobControl,
        now_ms: u64,
    ) -> Result<(JobId, JobReporter<P, R>), JobAdmissionError> {
        let mut state = self.inner.state.lock().unwrap();

        // 1. Registry open?
        if state.shutting_down {
            return Err(JobAdmissionError::ShuttingDown);
        }

        // 2. Authority/owner/kind match
        Self::validate_admission(&admission)?;

        // 3. Prune eligible terminal entries
        Self::prune_terminals(&mut state, now_ms);

        // 4. Terminal capacity
        let terminal_count = state
            .jobs
            .values()
            .filter(|r| r.state.is_terminal())
            .count();
        if terminal_count >= MAX_TERMINAL_JOBS {
            return Err(JobAdmissionError::TerminalCapacity {
                limit: MAX_TERMINAL_JOBS,
            });
        }

        // 5. Active capacity
        let active_count = state.jobs.values().filter(|r| r.state.is_active()).count();
        if active_count >= MAX_ACTIVE_JOBS {
            return Err(JobAdmissionError::ActiveLimit {
                limit: MAX_ACTIVE_JOBS,
            });
        }

        // 6. Reserved result-slot capacity (active jobs + uncollected results)
        let reserved_slots = state
            .jobs
            .values()
            .filter(|r| {
                r.state.is_active() || (r.state == JobState::Succeeded && r.progress.is_some())
            })
            .count();
        if reserved_slots >= MAX_UNCOLLECTED_RESULT_SLOTS {
            return Err(JobAdmissionError::ResultCapacity {
                limit: MAX_UNCOLLECTED_RESULT_SLOTS,
            });
        }

        // 7. Allocate
        let id = JobId::new();
        let record = JobRecord {
            kind: admission.kind,
            execution_class: admission.execution_class,
            owner: admission.owner,
            state: JobState::Starting,
            control,
            progress: None,
            revision: 1,
            created_at_ms: now_ms,
        };

        state.jobs.insert(id.clone(), record);
        state.watch_revision += 1;
        let _ = self.inner.watch_tx.send(state.watch_revision);

        let reporter = JobReporter {
            inner: Arc::clone(&self.inner),
            job_id: id.clone(),
            terminal_reported: false,
        };

        Ok((id, reporter))
    }

    /// Get a snapshot of a single job.
    pub fn snapshot(&self, id: &JobId) -> Option<JobSnapshot<P>>
    where
        P: Clone,
    {
        let state = self.inner.state.lock().unwrap();
        state.jobs.get(id).map(|record| JobSnapshot {
            id: id.clone(),
            kind: record.kind,
            execution_class: record.execution_class,
            owner: record.owner.clone(),
            state: record.state,
            progress: record.progress.clone(),
            revision: record.revision,
            created_at_ms: record.created_at_ms,
        })
    }

    /// List all active job IDs.
    pub fn list(&self) -> Vec<JobId> {
        let state = self.inner.state.lock().unwrap();
        state
            .jobs
            .iter()
            .filter(|(_, r)| r.state.is_active())
            .map(|(id, _)| id.clone())
            .collect()
    }

    // ---- private helpers ----

    fn validate_admission(admission: &JobAdmission) -> Result<(), JobAdmissionError> {
        match &admission.authority {
            JobAuthoritySource::DirectUserAction(action) => {
                // Direct user action requires DirectProductAction owner
                match &admission.owner {
                    JobOwner::DirectProductAction { .. } => {
                        // Kind must match action
                        match action {
                            DirectUserAction::ActionGuideVideoImport => match admission.kind {
                                JobKind::ActionGuideVideoImport => Ok(()),
                            },
                        }
                    }
                    JobOwner::ProductTask(_) => Err(JobAdmissionError::OwnerAuthorityMismatch),
                }
            }
            JobAuthoritySource::AgentTask { .. } => {
                // V1: always reject
                Err(JobAdmissionError::UnsupportedAuthoritySource)
            }
        }
    }

    fn prune_terminals(state: &mut RegistryState<P>, now_ms: u64) {
        state.jobs.retain(|_, record| {
            if record.state.is_terminal() {
                let elapsed = now_ms.saturating_sub(record.created_at_ms);
                elapsed < TERMINAL_TTL_MS
            } else {
                true
            }
        });
    }
}

// ========================================================================
// JobReporter (worker's mutable handle to exactly one job)
// ========================================================================

/// Worker capability to update exactly one job. Cannot mutate another job
/// or admit new work.
pub struct JobReporter<P, R> {
    inner: Arc<Inner<P, R>>,
    job_id: JobId,
    terminal_reported: bool,
}

impl<P, R> fmt::Debug for JobReporter<P, R> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("JobReporter")
            .field("job_id", &self.job_id)
            .field("terminal_reported", &self.terminal_reported)
            .finish()
    }
}

impl<P, R> JobReporter<P, R> {
    /// Transition from Starting to Running.
    pub fn mark_running(&mut self, _now_ms: u64) -> Result<(), JobTransitionError> {
        if self.terminal_reported {
            return Err(JobTransitionError::StaleReporter);
        }

        let mut state = self.inner.state.lock().unwrap();
        let record = state
            .jobs
            .get_mut(&self.job_id)
            .ok_or(JobTransitionError::NotFound)?;

        if record.state != JobState::Starting {
            return Err(JobTransitionError::InvalidTransition {
                from: record.state,
                operation: "mark_running",
            });
        }

        record.state = JobState::Running;
        record.revision += 1;
        state.watch_revision += 1;
        let _ = self.inner.watch_tx.send(state.watch_revision);

        Ok(())
    }
}

// ========================================================================
// Tests
// ========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::authority::{AuthorityBinding, AuthoritySnapshot, DisclosureCeiling};
    use crate::product_task::{AnnotationStateV1, DocumentContentBinding};
    use std::collections::BTreeSet;

    // ---- fixed helpers ----

    fn task_id() -> ProductTaskId {
        ProductTaskId::parse("task-00000000-0000-4000-8000-000000000001").unwrap()
    }

    fn run_id() -> RunId {
        RunId::parse("run-00000000-0000-4000-8000-000000000002").unwrap()
    }

    fn direct_admission(nonce: u64) -> JobAdmission {
        JobAdmission::action_guide_video_import(nonce)
    }

    fn no_op_control() -> JobControl {
        JobControl::new(|| {})
    }

    fn authority_fixture(
        task_id: ProductTaskId,
        attempt_id: TaskAttemptId,
        run_id: RunId,
    ) -> AuthoritySnapshot {
        let state = AnnotationStateV1 {
            width: 100,
            height: 80,
            state_id: 1,
            annotations: vec![],
        };
        let document = DocumentContentBinding::new([0xAB_u8; 32], &state, 1).unwrap();
        AuthoritySnapshot::new(
            AuthorityBinding::new(task_id, attempt_id, run_id, document),
            "job-test-policy-v1".into(),
            DisclosureCeiling::OcrLayoutOnly,
            false,
            BTreeSet::new(),
            BTreeSet::new(),
        )
        .unwrap()
    }

    // ---- Step 1: identity and admission tests ----

    #[test]
    fn admitted_job_has_typed_unique_identity_and_exact_metadata() {
        let registry = LiveJobRegistry::<u32, String>::new();
        let (first, _) = registry
            .admit(direct_admission(7), no_op_control(), 100)
            .unwrap();
        let (second, _) = registry
            .admit(direct_admission(8), no_op_control(), 101)
            .unwrap();

        assert_ne!(first, second);
        assert!(first.as_str().starts_with("job-"));
        let snapshot = registry.snapshot(&first).unwrap();
        assert_eq!(snapshot.kind(), JobKind::ActionGuideVideoImport);
        assert_eq!(
            snapshot.execution_class(),
            JobExecutionClass::LocalWorkerWithChildProcesses
        );
        assert_eq!(
            snapshot.owner(),
            &JobOwner::DirectProductAction {
                surface: ProductSurface::ActionGuideHome,
                operation_nonce: 7,
            }
        );
        assert_eq!(snapshot.state(), JobState::Starting);
        assert_eq!(snapshot.revision(), 1);
    }

    #[test]
    fn agent_task_authority_is_represented_but_rejected_before_allocation() {
        let registry = LiveJobRegistry::<u32, String>::new();
        let authority = authority_fixture(task_id(), TaskAttemptId::new(1), run_id());
        let task = JobTaskRef::new(task_id(), TaskAttemptId::new(1), run_id());
        let admission = JobAdmission::agent_task(
            JobKind::ActionGuideVideoImport,
            JobExecutionClass::LocalWorkerWithChildProcesses,
            authority,
            task,
        );

        assert_eq!(
            registry.admit(admission, no_op_control(), 100).unwrap_err(),
            JobAdmissionError::UnsupportedAuthoritySource
        );
        assert!(registry.list().is_empty());
    }

    #[test]
    fn direct_authority_cannot_claim_product_task_ownership() {
        let admission = JobAdmission::for_test(
            JobKind::ActionGuideVideoImport,
            JobExecutionClass::LocalWorkerWithChildProcesses,
            JobOwner::ProductTask(JobTaskRef::new(task_id(), TaskAttemptId::new(1), run_id())),
            JobAuthoritySource::DirectUserAction(DirectUserAction::ActionGuideVideoImport),
        );
        let registry = LiveJobRegistry::<u32, String>::new();

        assert_eq!(
            registry.admit(admission, no_op_control(), 100).unwrap_err(),
            JobAdmissionError::OwnerAuthorityMismatch
        );
        assert!(registry.list().is_empty());
    }

    // ---- Step 2: active-capacity and transition tests ----

    #[test]
    fn fifth_active_job_is_rejected_without_evicting_active_work() {
        let registry = LiveJobRegistry::<u32, String>::new();
        let mut ids = Vec::new();
        for nonce in 0..4 {
            ids.push(
                registry
                    .admit(direct_admission(nonce), no_op_control(), nonce)
                    .unwrap()
                    .0,
            );
        }

        assert_eq!(
            registry
                .admit(direct_admission(4), no_op_control(), 4)
                .unwrap_err(),
            JobAdmissionError::ActiveLimit { limit: 4 }
        );
        assert_eq!(registry.list().len(), 4);
        assert!(ids.iter().all(|id| registry.snapshot(id).is_some()));
    }

    #[test]
    fn reporter_moves_starting_to_running_once() {
        let registry = LiveJobRegistry::<u32, String>::new();
        let (id, mut reporter) = registry
            .admit(direct_admission(7), no_op_control(), 100)
            .unwrap();

        reporter.mark_running(101).unwrap();
        assert_eq!(registry.snapshot(&id).unwrap().state(), JobState::Running);
        assert_eq!(registry.snapshot(&id).unwrap().revision(), 2);
        assert_eq!(
            reporter.mark_running(102).unwrap_err(),
            JobTransitionError::InvalidTransition {
                from: JobState::Running,
                operation: "mark_running",
            }
        );
    }
}
