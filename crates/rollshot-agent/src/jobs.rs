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

use std::collections::{HashMap, HashSet, VecDeque};
use std::fmt;
use std::hash::{Hash, Hasher};
use std::sync::{Arc, Mutex};

use tracing::{event, Level};

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

impl Clone for JobControl {
    fn clone(&self) -> Self {
        Self {
            cancel_fn: Arc::clone(&self.cancel_fn),
        }
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
// Failure category (closed V1)
// ========================================================================

/// Stable failure category for terminal jobs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum JobFailureCategory {
    ProbeFailed,
    MissingVideoStream,
    InvalidVideoMetadata,
    DecoderUnavailable,
    DecodeFailed,
    EvidenceMissing,
    ScratchIo,
    ResourceLimit,
    WorkerAbandoned,
    WorkerPanic,
}

// ========================================================================
// Cancellation outcome
// ========================================================================

/// Typed outcome from a cancellation request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JobCancelOutcome {
    Requested,
    AlreadyRequested,
    AlreadyTerminal,
    NotFound,
}

// ========================================================================
// Diagnostics
// ========================================================================

/// Closed diagnostic category.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum JobDiagnosticCategory {
    Lifecycle,
    Worker,
    Cleanup,
}

/// Bounded, code-owned diagnostic entry.
///
/// Message is a static string — no runtime paths, PID, or process output.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JobDiagnostic {
    category: JobDiagnosticCategory,
    message: &'static str,
}

impl JobDiagnostic {
    pub fn new(
        category: JobDiagnosticCategory,
        message: &'static str,
    ) -> Result<Self, JobDiagnosticError> {
        if message.is_empty() {
            return Err(JobDiagnosticError::Empty);
        }
        if message.len() > MAX_DIAGNOSTIC_BYTES {
            return Err(JobDiagnosticError::TooLong {
                limit: MAX_DIAGNOSTIC_BYTES,
            });
        }
        Ok(Self { category, message })
    }

    pub fn category(&self) -> JobDiagnosticCategory {
        self.category
    }

    pub fn message(&self) -> &'static str {
        self.message
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum JobDiagnosticError {
    #[error("job diagnostic message must not be empty")]
    Empty,
    #[error("job diagnostic exceeds {limit} bytes")]
    TooLong { limit: usize },
}

// ========================================================================
// Collection errors
// ========================================================================

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum JobCollectError {
    #[error("job not found")]
    NotFound,
    #[error("job did not succeed")]
    NotSucceeded,
    #[error("job result was already collected")]
    AlreadyCollected,
    #[error("job result expired")]
    ResultExpired,
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
    failure_category: Option<JobFailureCategory>,
    revision: u64,
    created_at_ms: u64,
    updated_at_ms: u64,
    terminal_at_ms: Option<u64>,
    cancelled_at_ms: Option<u64>,
    result_collected: bool,
    diagnostics: VecDeque<JobDiagnostic>,
    dropped_diagnostics: u32,
}

impl<P: fmt::Debug> fmt::Debug for JobSnapshot<P> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("JobSnapshot")
            .field("id", &self.id)
            .field("kind", &self.kind)
            .field("execution_class", &self.execution_class)
            .field("owner", &self.owner)
            .field("state", &self.state)
            .field("progress", &self.progress)
            .field("failure_category", &self.failure_category)
            .field("revision", &self.revision)
            .field("created_at_ms", &self.created_at_ms)
            .field("updated_at_ms", &self.updated_at_ms)
            .field("terminal_at_ms", &self.terminal_at_ms)
            .field("cancelled_at_ms", &self.cancelled_at_ms)
            .field("result_collected", &self.result_collected)
            .field("diagnostics_len", &self.diagnostics.len())
            .field("dropped_diagnostics", &self.dropped_diagnostics)
            // NOTE: R, callback, path, PID, child handle, and raw log
            // are deliberately absent from Debug output.
            .finish()
    }
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

    pub fn failure_category(&self) -> Option<JobFailureCategory> {
        self.failure_category
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn created_at_ms(&self) -> u64 {
        self.created_at_ms
    }

    pub fn updated_at_ms(&self) -> u64 {
        self.updated_at_ms
    }

    pub fn terminal_at_ms(&self) -> Option<u64> {
        self.terminal_at_ms
    }

    pub fn cancelled_at_ms(&self) -> Option<u64> {
        self.cancelled_at_ms
    }

    pub fn result_collected(&self) -> bool {
        self.result_collected
    }

    pub fn diagnostics(&self) -> &VecDeque<JobDiagnostic> {
        &self.diagnostics
    }

    pub fn dropped_diagnostics(&self) -> u32 {
        self.dropped_diagnostics
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
    failure_category: Option<JobFailureCategory>,
    result: Option</* R */ Box<dyn std::any::Any + Send>>,
    result_collected: bool,
    result_expired: bool,
    revision: u64,
    created_at_ms: u64,
    updated_at_ms: u64,
    terminal_at_ms: Option<u64>,
    cancelled_at_ms: Option<u64>,
    diagnostics: VecDeque<JobDiagnostic>,
    dropped_diagnostics: u32,
}

// ========================================================================
// Registry inner state
// ========================================================================

struct RegistryState<P> {
    jobs: HashMap<JobId, JobRecord<P>>,
    shutting_down: bool,
    watch_revision: u64,
    tombstones: HashSet<String>,
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
// JobWatch (coalescible notification)
// ========================================================================

/// Coalescible watch handle. Notifications are hints; callers must query
/// snapshots for terminal truth.
#[derive(Clone)]
pub struct JobWatch {
    registry_key: u64,
    receiver: watch::Receiver<u64>,
}

impl fmt::Debug for JobWatch {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("JobWatch")
            .field("registry_key", &self.registry_key)
            // Receiver internals are omitted to avoid leaking state.
            .finish()
    }
}

impl Hash for JobWatch {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.registry_key.hash(state);
    }
}

impl PartialEq for JobWatch {
    fn eq(&self, other: &Self) -> bool {
        self.registry_key == other.registry_key
    }
}

impl Eq for JobWatch {}

impl JobWatch {
    /// Access the underlying receiver for change detection.
    pub fn receiver(&mut self) -> watch::Receiver<u64> {
        self.receiver.clone()
    }

    /// The registry watch key at construction time.
    pub fn registry_key(&self) -> u64 {
        self.registry_key
    }
}

// ========================================================================
// JobObserver (read-only view)
// ========================================================================

/// Read-only observation handle. Does not admit or mutate jobs.
#[derive(Clone)]
pub struct JobObserver<P, R> {
    inner: Arc<Inner<P, R>>,
}

impl<P, R> fmt::Debug for JobObserver<P, R> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("JobObserver")
            // Inner state is omitted to prevent leaking registry contents.
            .finish()
    }
}

impl<P: Clone, R: 'static> JobObserver<P, R> {
    /// Get a snapshot of a single job.
    pub fn snapshot(&self, id: &JobId) -> Option<JobSnapshot<P>> {
        let state = self.inner.state.lock().unwrap();
        Self::build_snapshot(&state, id)
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

    fn build_snapshot(state: &RegistryState<P>, id: &JobId) -> Option<JobSnapshot<P>> {
        state.jobs.get(id).map(|record| JobSnapshot {
            id: id.clone(),
            kind: record.kind,
            execution_class: record.execution_class,
            owner: record.owner.clone(),
            state: record.state,
            progress: record.progress.clone(),
            failure_category: record.failure_category,
            revision: record.revision,
            created_at_ms: record.created_at_ms,
            updated_at_ms: record.updated_at_ms,
            terminal_at_ms: record.terminal_at_ms,
            cancelled_at_ms: record.cancelled_at_ms,
            result_collected: record.result_collected,
            diagnostics: record.diagnostics.clone(),
            dropped_diagnostics: record.dropped_diagnostics,
        })
    }
}

// ========================================================================
// LiveJobRegistry
// ========================================================================

/// Process-local live job registry. Generic over structured progress `P`
/// and successful result `R`.
pub struct LiveJobRegistry<P, R: Send + 'static> {
    inner: Arc<Inner<P, R>>,
}

impl<P, R: Send + 'static> LiveJobRegistry<P, R> {
    /// Create a new empty registry.
    pub fn new() -> Self {
        let (watch_tx, _) = watch::channel(0);
        Self {
            inner: Arc::new(Inner {
                state: Mutex::new(RegistryState {
                    jobs: HashMap::new(),
                    shutting_down: false,
                    watch_revision: 0,
                    tombstones: HashSet::new(),
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
            event!(target: "rollshot::agent::jobs", Level::WARN,
                kind = ?admission.kind,
                "admission_rejected_shutting_down"
            );
            return Err(JobAdmissionError::ShuttingDown);
        }

        // 2. Authority/owner/kind match
        Self::validate_admission(&admission)?;

        // 3. Prune eligible terminal entries (make room for new admission)
        Self::prune_terminals(&mut state, now_ms, true);

        // 4. Terminal capacity
        let terminal_count = state
            .jobs
            .values()
            .filter(|r| r.state.is_terminal())
            .count();
        if terminal_count >= MAX_TERMINAL_JOBS {
            event!(target: "rollshot::agent::jobs", Level::WARN,
                kind = ?admission.kind,
                terminal_count = terminal_count,
                "admission_rejected_terminal_capacity"
            );
            return Err(JobAdmissionError::TerminalCapacity {
                limit: MAX_TERMINAL_JOBS,
            });
        }

        // 5. Active capacity
        let active_count = state.jobs.values().filter(|r| r.state.is_active()).count();
        if active_count >= MAX_ACTIVE_JOBS {
            event!(target: "rollshot::agent::jobs", Level::WARN,
                kind = ?admission.kind,
                active_count = active_count,
                "admission_rejected_active_limit"
            );
            return Err(JobAdmissionError::ActiveLimit {
                limit: MAX_ACTIVE_JOBS,
            });
        }

        // 6. Reserved result-slot capacity (active jobs + uncollected successful results)
        let reserved_slots = state
            .jobs
            .values()
            .filter(|r| r.state.is_active() || r.result.is_some())
            .count();
        if reserved_slots >= MAX_UNCOLLECTED_RESULT_SLOTS {
            event!(target: "rollshot::agent::jobs", Level::WARN,
                kind = ?admission.kind,
                reserved_slots = reserved_slots,
                "admission_rejected_result_capacity"
            );
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
            failure_category: None,
            result: None,
            result_collected: false,
            result_expired: false,
            revision: 1,
            created_at_ms: now_ms,
            updated_at_ms: now_ms,
            terminal_at_ms: None,
            cancelled_at_ms: None,
            diagnostics: VecDeque::new(),
            dropped_diagnostics: 0,
        };

        state.jobs.insert(id.clone(), record);
        state.watch_revision += 1;
        let _ = self.inner.watch_tx.send(state.watch_revision);

        event!(target: "rollshot::agent::jobs", Level::INFO,
            job_id = %id.as_str(),
            kind = ?admission.kind,
            revision = 1u64,
            "admitted"
        );

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
        Self::build_snapshot(&state, id)
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

    /// Request cancellation of an active job.
    ///
    /// Returns `Requested` on first request, `AlreadyRequested` if already
    /// cancelling, `AlreadyTerminal` if already done, `NotFound` if unknown.
    /// The cancellation callback is invoked outside the registry lock.
    pub fn cancel(&self, id: &JobId, now_ms: u64) -> JobCancelOutcome {
        let (outcome, callback) = {
            let mut state = self.inner.state.lock().unwrap();
            let record = match state.jobs.get_mut(id) {
                Some(r) => r,
                None => return JobCancelOutcome::NotFound,
            };

            match record.state {
                s if s.is_terminal() => (JobCancelOutcome::AlreadyTerminal, None),
                JobState::Cancelling => (JobCancelOutcome::AlreadyRequested, None),
                JobState::Starting | JobState::Running => {
                    let ctrl = record.control.clone();
                    record.state = JobState::Cancelling;
                    record.cancelled_at_ms = Some(now_ms);
                    record.revision += 1;
                    state.watch_revision += 1;
                    let _ = self.inner.watch_tx.send(state.watch_revision);
                    (JobCancelOutcome::Requested, Some(ctrl))
                }
                _ => unreachable!("all JobState variants covered"),
            }
        };

        // Invoke callback outside lock.
        if let Some(ctrl) = callback {
            event!(target: "rollshot::agent::jobs", Level::INFO,
                job_id = %id.as_str(),
                "cancellation_requested"
            );
            ctrl.invoke();
        }

        outcome
    }

    /// Collect the successful result of a job exactly once.
    pub fn collect(&self, id: &JobId, now_ms: u64) -> Result<R, JobCollectError> {
        let mut state = self.inner.state.lock().unwrap();

        // Prune expired terminals first.
        Self::prune_terminals(&mut state, now_ms, false);

        let record = match state.jobs.get_mut(id) {
            Some(r) => r,
            None => {
                // Check tombstone set: the record was evicted with an
                // uncollected result, so ResultExpired is the correct answer.
                if state.tombstones.contains(id.as_str()) {
                    return Err(JobCollectError::ResultExpired);
                }
                return Err(JobCollectError::NotFound);
            }
        };

        if record.state != JobState::Succeeded {
            return Err(JobCollectError::NotSucceeded);
        }

        if record.result_collected {
            return Err(JobCollectError::AlreadyCollected);
        }

        match record.result.take() {
            Some(boxed) => {
                record.result_collected = true;
                event!(target: "rollshot::agent::jobs", Level::INFO,
                    job_id = %id.as_str(),
                    kind = ?record.kind,
                    revision = record.revision,
                    "result_collected"
                );
                // Downcast from Box<dyn Any + Send> to R.
                Ok(*boxed.downcast::<R>().expect("result type mismatch"))
            }
            None => {
                record.result_expired = true;
                event!(target: "rollshot::agent::jobs", Level::INFO,
                    job_id = %id.as_str(),
                    kind = ?record.kind,
                    revision = record.revision,
                    "result_expired"
                );
                Err(JobCollectError::ResultExpired)
            }
        }
    }

    /// Create a read-only observation handle.
    pub fn observer(&self) -> JobObserver<P, R> {
        JobObserver {
            inner: Arc::clone(&self.inner),
        }
    }

    /// Create a coalescible watch handle.
    pub fn watch(&self) -> JobWatch {
        let state = self.inner.state.lock().unwrap();
        JobWatch {
            registry_key: state.watch_revision,
            receiver: self.inner.watch_tx.subscribe(),
        }
    }

    /// Manually trigger terminal pruning.
    pub fn prune(&self, now_ms: u64) {
        let mut state = self.inner.state.lock().unwrap();
        Self::prune_terminals(&mut state, now_ms, false);
    }

    /// Request cancellation for all active jobs and reject new admission.
    ///
    /// Returns the IDs of all jobs that had cancellation requested.
    pub fn shutdown(&self, now_ms: u64) -> Vec<JobId> {
        let (requested_ids, callbacks) = {
            let mut state = self.inner.state.lock().unwrap();
            state.shutting_down = true;

            let mut requested_ids = Vec::new();
            let mut callbacks = Vec::new();

            for (id, record) in state.jobs.iter_mut() {
                match record.state {
                    JobState::Starting | JobState::Running => {
                        record.state = JobState::Cancelling;
                        record.cancelled_at_ms = Some(now_ms);
                        record.revision += 1;
                        requested_ids.push(id.clone());
                        callbacks.push(record.control.clone());
                    }
                    JobState::Cancelling => {
                        // Already cancelling; don't re-invoke.
                    }
                    _ => {
                        // Already terminal.
                    }
                }
            }

            if !requested_ids.is_empty() {
                state.watch_revision += 1;
                let _ = self.inner.watch_tx.send(state.watch_revision);
            }

            (requested_ids, callbacks)
        };

        // Invoke callbacks outside lock.
        for ctrl in callbacks {
            ctrl.invoke();
        }

        event!(target: "rollshot::agent::jobs", Level::INFO,
            requested_count = requested_ids.len(),
            "shutdown"
        );

        requested_ids
    }

    // ---- private helpers ----

    fn build_snapshot(state: &RegistryState<P>, id: &JobId) -> Option<JobSnapshot<P>>
    where
        P: Clone,
    {
        state.jobs.get(id).map(|record| JobSnapshot {
            id: id.clone(),
            kind: record.kind,
            execution_class: record.execution_class,
            owner: record.owner.clone(),
            state: record.state,
            progress: record.progress.clone(),
            failure_category: record.failure_category,
            revision: record.revision,
            created_at_ms: record.created_at_ms,
            updated_at_ms: record.updated_at_ms,
            terminal_at_ms: record.terminal_at_ms,
            cancelled_at_ms: record.cancelled_at_ms,
            result_collected: record.result_collected,
            diagnostics: record.diagnostics.clone(),
            dropped_diagnostics: record.dropped_diagnostics,
        })
    }

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

    /// Prune expired terminal records. If `make_room` is true and terminal cap
    /// is reached, prune the oldest collected or result-free terminal before an
    /// uncollected success. An uncollected successful result may be dropped only
    /// at TTL expiry.
    fn prune_terminals(state: &mut RegistryState<P>, now_ms: u64, make_room: bool) {
        // 1. At TTL: drop results from uncollected successes (mark expired).
        //    Terminal records persist until cap eviction or owner drop.
        for (id, record) in state.jobs.iter_mut() {
            if !record.state.is_terminal() {
                continue;
            }
            let terminal_at = record.terminal_at_ms.unwrap_or(record.created_at_ms);
            let elapsed = now_ms.saturating_sub(terminal_at);
            if elapsed >= TERMINAL_TTL_MS {
                if record.state == JobState::Succeeded
                    && record.result.is_some()
                    && !record.result_collected
                {
                    // Drop the result and mark expired. Tombstone so collect
                    // can return ResultExpired even after cap eviction.
                    record.result = None;
                    record.result_expired = true;
                    if state.tombstones.len() < MAX_TERMINAL_JOBS {
                        state.tombstones.insert(id.as_str().to_owned());
                    }
                }
            }
        }

        // 2. If make_room and at terminal cap, evict oldest collected/result-free first.
        let terminal_count = state
            .jobs
            .values()
            .filter(|r| r.state.is_terminal())
            .count();

        if make_room && terminal_count >= MAX_TERMINAL_JOBS {
            // Collect terminal job IDs sorted by terminal_at_ms ascending.
            let mut terminal_ids: Vec<(JobId, u64, bool, bool)> = state
                .jobs
                .iter()
                .filter(|(_, r)| r.state.is_terminal())
                .map(|(id, r)| {
                    let has_uncollected_result = r.state == JobState::Succeeded
                        && (r.result.is_some() || r.result_expired)
                        && !r.result_collected;
                    let is_succeeded = r.state == JobState::Succeeded && !r.result_collected;
                    (
                        id.clone(),
                        r.terminal_at_ms.unwrap_or(r.created_at_ms),
                        has_uncollected_result,
                        is_succeeded,
                    )
                })
                .collect();

            terminal_ids.sort_by_key(|(_, ts, _, _)| *ts);

            let excess = terminal_count - MAX_TERMINAL_JOBS + 1;
            let mut removed = 0;

            // First pass: remove collected or result-free terminals.
            for (id, _, has_uncollected, _) in &terminal_ids {
                if removed >= excess {
                    break;
                }
                if !has_uncollected {
                    state.jobs.remove(id);
                    removed += 1;
                }
            }

            // Second pass: if still over, remove uncollected (oldest first).
            // Tombstone so collect can return ResultExpired.
            for (id, _, has_uncollected, is_succeeded) in &terminal_ids {
                if removed >= excess {
                    break;
                }
                if *has_uncollected && state.jobs.contains_key(id) {
                    if *is_succeeded && state.tombstones.len() < MAX_TERMINAL_JOBS {
                        state.tombstones.insert(id.as_str().to_owned());
                    }
                    state.jobs.remove(id);
                    removed += 1;
                }
            }
        }
    }
}

impl<P, R: Send + 'static> Default for LiveJobRegistry<P, R> {
    fn default() -> Self {
        Self::new()
    }
}

impl<P, R: Send + 'static> Drop for LiveJobRegistry<P, R> {
    fn drop(&mut self) {
        // Idempotent shutdown: sets shutting_down, requests cancellation of
        // any remaining active jobs, and invokes their callbacks.
        // Reporter/observer Arcs may outlive us, but shutdown state remains
        // visible to them.
        self.shutdown(0);
    }
}

// ========================================================================
// JobReporter (worker's mutable handle to exactly one job)
// ========================================================================

/// Worker capability to update exactly one job. Cannot mutate another job
/// or admit new work.
pub struct JobReporter<P, R: Send + 'static> {
    inner: Arc<Inner<P, R>>,
    job_id: JobId,
    terminal_reported: bool,
}

impl<P, R: Send + 'static> fmt::Debug for JobReporter<P, R> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("JobReporter")
            .field("job_id", &self.job_id)
            .field("terminal_reported", &self.terminal_reported)
            .finish()
    }
}

impl<P, R: Send + 'static> JobReporter<P, R> {
    /// Transition from Starting to Running.
    pub fn mark_running(&mut self, now_ms: u64) -> Result<(), JobTransitionError> {
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
        record.updated_at_ms = now_ms;
        record.revision += 1;
        let rev = record.revision;
        state.watch_revision += 1;
        let _ = self.inner.watch_tx.send(state.watch_revision);

        event!(target: "rollshot::agent::jobs", Level::INFO,
            job_id = %self.job_id.as_str(),
            state = ?JobState::Running,
            revision = rev,
            "running"
        );

        Ok(())
    }

    /// Report structured progress. Only the latest value is retained.
    pub fn report_progress(&mut self, progress: P, now_ms: u64) -> Result<(), JobTransitionError> {
        if self.terminal_reported {
            return Err(JobTransitionError::StaleReporter);
        }

        let mut state = self.inner.state.lock().unwrap();
        let record = state
            .jobs
            .get_mut(&self.job_id)
            .ok_or(JobTransitionError::NotFound)?;

        if record.state.is_terminal() {
            return Err(JobTransitionError::InvalidTransition {
                from: record.state,
                operation: "report_progress",
            });
        }

        record.progress = Some(progress);
        record.updated_at_ms = now_ms;
        record.revision += 1;
        state.watch_revision += 1;
        let _ = self.inner.watch_tx.send(state.watch_revision);

        Ok(())
    }

    /// Append a bounded diagnostic entry. Overflow drops the oldest entry.
    pub fn append_diagnostic(
        &mut self,
        diagnostic: JobDiagnostic,
        now_ms: u64,
    ) -> Result<(), JobTransitionError> {
        if self.terminal_reported {
            return Err(JobTransitionError::StaleReporter);
        }

        let mut state = self.inner.state.lock().unwrap();
        let record = state
            .jobs
            .get_mut(&self.job_id)
            .ok_or(JobTransitionError::NotFound)?;

        if record.state.is_terminal() {
            return Err(JobTransitionError::InvalidTransition {
                from: record.state,
                operation: "append_diagnostic",
            });
        }

        if record.diagnostics.len() >= MAX_DIAGNOSTIC_ENTRIES {
            record.diagnostics.remove(0);
            record.dropped_diagnostics += 1;
        }
        record.diagnostics.push_back(diagnostic);
        record.updated_at_ms = now_ms;
        record.revision += 1;
        state.watch_revision += 1;
        let _ = self.inner.watch_tx.send(state.watch_revision);

        Ok(())
    }

    /// Report successful completion with a result.
    ///
    /// If the job is currently `Cancelling`, the result is dropped and the
    /// job terminalizes as `Cancelled` instead.
    pub fn succeed(&mut self, result: R, now_ms: u64) -> Result<(), JobTransitionError> {
        self.terminal_reported = true;

        let mut state = self.inner.state.lock().unwrap();
        let record = state
            .jobs
            .get_mut(&self.job_id)
            .ok_or(JobTransitionError::NotFound)?;

        if record.state.is_terminal() {
            self.terminal_reported = false;
            return Err(JobTransitionError::TerminalConflict);
        }

        if record.state == JobState::Cancelling {
            // Success while cancelling: drop result, terminalize as Cancelled.
            drop(result);
            record.state = JobState::Cancelled;
            record.terminal_at_ms = Some(now_ms);
            record.updated_at_ms = now_ms;
            record.revision += 1;
            let rev = record.revision;
            state.watch_revision += 1;
            let _ = self.inner.watch_tx.send(state.watch_revision);
            event!(target: "rollshot::agent::jobs", Level::INFO,
                job_id = %self.job_id.as_str(),
                state = ?JobState::Cancelled,
                revision = rev,
                "terminal"
            );
            return Ok(());
        }

        // Normal success path: store result and terminalize.
        record.state = JobState::Succeeded;
        record.result = Some(Box::new(result));
        record.terminal_at_ms = Some(now_ms);
        record.updated_at_ms = now_ms;
        record.revision += 1;
        let rev = record.revision;
        state.watch_revision += 1;
        let _ = self.inner.watch_tx.send(state.watch_revision);

        event!(target: "rollshot::agent::jobs", Level::INFO,
            job_id = %self.job_id.as_str(),
            state = ?JobState::Succeeded,
            revision = rev,
            "terminal"
        );

        Ok(())
    }

    /// Report failure with a category.
    pub fn fail(
        &mut self,
        category: JobFailureCategory,
        now_ms: u64,
    ) -> Result<(), JobTransitionError> {
        self.terminal_reported = true;

        let mut state = self.inner.state.lock().unwrap();
        let record = state
            .jobs
            .get_mut(&self.job_id)
            .ok_or(JobTransitionError::NotFound)?;

        if record.state.is_terminal() {
            self.terminal_reported = false;
            return Err(JobTransitionError::TerminalConflict);
        }

        record.state = JobState::Failed;
        record.failure_category = Some(category);
        record.terminal_at_ms = Some(now_ms);
        record.updated_at_ms = now_ms;
        record.revision += 1;
        let rev = record.revision;
        state.watch_revision += 1;
        let _ = self.inner.watch_tx.send(state.watch_revision);

        event!(target: "rollshot::agent::jobs", Level::INFO,
            job_id = %self.job_id.as_str(),
            state = ?JobState::Failed,
            failure_category = ?category,
            revision = rev,
            "terminal"
        );

        Ok(())
    }

    /// Report confirmed cancellation. Only valid from `Cancelling` state.
    ///
    /// This should be called after the worker has completed concrete cleanup
    /// (child process reaping, scratch removal, etc.).
    pub fn cancelled(&mut self, now_ms: u64) -> Result<(), JobTransitionError> {
        self.terminal_reported = true;

        let mut state = self.inner.state.lock().unwrap();
        let record = state
            .jobs
            .get_mut(&self.job_id)
            .ok_or(JobTransitionError::NotFound)?;

        if record.state.is_terminal() {
            self.terminal_reported = false;
            return Err(JobTransitionError::TerminalConflict);
        }

        if record.state != JobState::Cancelling {
            self.terminal_reported = false;
            return Err(JobTransitionError::InvalidTransition {
                from: record.state,
                operation: "cancelled",
            });
        }

        record.state = JobState::Cancelled;
        record.terminal_at_ms = Some(now_ms);
        record.updated_at_ms = now_ms;
        record.revision += 1;
        let rev = record.revision;
        state.watch_revision += 1;
        let _ = self.inner.watch_tx.send(state.watch_revision);

        event!(target: "rollshot::agent::jobs", Level::INFO,
            job_id = %self.job_id.as_str(),
            state = ?JobState::Cancelled,
            revision = rev,
            "terminal"
        );

        Ok(())
    }
}

/// Drop impl: if the reporter is dropped without a terminal report,
/// mark the job as `Failed(WorkerAbandoned)`.
///
/// If the reporter is dropped while `Cancelling`, mark as `Cancelled`
/// (reporter-stack destruction follows concrete resource owners).
impl<P, R: Send + 'static> Drop for JobReporter<P, R> {
    fn drop(&mut self) {
        if self.terminal_reported {
            return;
        }

        // We need to determine the terminal state without holding a borrow
        // across the lock boundary.
        let mut state = self.inner.state.lock().unwrap();
        let record = match state.jobs.get_mut(&self.job_id) {
            Some(r) => r,
            None => return,
        };

        if record.state.is_terminal() {
            return;
        }

        if record.state == JobState::Cancelling {
            // Drop while cancelling is confirmation of cancellation.
            record.state = JobState::Cancelled;
            record.terminal_at_ms = Some(record.updated_at_ms);
            record.revision += 1;
            let rev = record.revision;
            state.watch_revision += 1;
            let _ = self.inner.watch_tx.send(state.watch_revision);
            event!(target: "rollshot::agent::jobs", Level::WARN,
                job_id = %self.job_id.as_str(),
                state = ?JobState::Cancelled,
                revision = rev,
                "terminal"
            );
        } else {
            // Starting or Running: worker abandoned.
            record.state = JobState::Failed;
            record.failure_category = Some(JobFailureCategory::WorkerAbandoned);
            record.terminal_at_ms = Some(record.updated_at_ms);
            record.revision += 1;
            let rev = record.revision;
            state.watch_revision += 1;
            let _ = self.inner.watch_tx.send(state.watch_revision);
            event!(target: "rollshot::agent::jobs", Level::WARN,
                job_id = %self.job_id.as_str(),
                state = ?JobState::Failed,
                failure_category = ?JobFailureCategory::WorkerAbandoned,
                revision = rev,
                "worker_abandoned"
            );
        }
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
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

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

    /// Drop-probe: sets an atomic flag when dropped.
    #[derive(Debug)]
    struct DropProbe(Arc<AtomicBool>);

    impl Drop for DropProbe {
        fn drop(&mut self) {
            self.0.store(true, Ordering::SeqCst);
        }
    }

    // ---- Task 2 tests (identity, admission, capacity, transitions) ----

    #[test]
    fn admitted_job_has_typed_unique_identity_and_exact_metadata() {
        let registry = LiveJobRegistry::<u32, String>::new();
        let (first, _r1) = registry
            .admit(direct_admission(7), no_op_control(), 100)
            .unwrap();
        let (second, _r2) = registry
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

    #[test]
    fn fifth_active_job_is_rejected_without_evicting_active_work() {
        let registry = LiveJobRegistry::<u32, String>::new();
        let mut ids = Vec::new();
        let mut reporters = Vec::new();
        for nonce in 0..4 {
            let (id, reporter) = registry
                .admit(direct_admission(nonce), no_op_control(), nonce)
                .unwrap();
            ids.push(id);
            reporters.push(reporter);
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

    // ---- Task 3: cancellation-honesty tests ----

    #[test]
    fn cancel_requests_control_but_worker_confirms_terminal() {
        let calls = Arc::new(AtomicUsize::new(0));
        let seen = calls.clone();
        let registry = LiveJobRegistry::<u32, String>::new();
        let (id, mut reporter) = registry
            .admit(
                direct_admission(7),
                JobControl::new(move || {
                    seen.fetch_add(1, Ordering::SeqCst);
                }),
                100,
            )
            .unwrap();
        reporter.mark_running(101).unwrap();

        assert_eq!(registry.cancel(&id, 102), JobCancelOutcome::Requested);
        assert_eq!(
            registry.snapshot(&id).unwrap().state(),
            JobState::Cancelling
        );
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert_eq!(
            registry.cancel(&id, 103),
            JobCancelOutcome::AlreadyRequested
        );
        assert_eq!(calls.load(Ordering::SeqCst), 1);

        reporter.cancelled(104).unwrap();
        assert_eq!(registry.snapshot(&id).unwrap().state(), JobState::Cancelled);
        assert_eq!(registry.cancel(&id, 105), JobCancelOutcome::AlreadyTerminal);
    }

    #[test]
    fn success_racing_with_cancel_is_dropped_and_becomes_cancelled() {
        let dropped = Arc::new(AtomicBool::new(false));
        let registry = LiveJobRegistry::<u32, DropProbe>::new();
        let (id, mut reporter) = registry
            .admit(direct_admission(7), no_op_control(), 100)
            .unwrap();
        reporter.mark_running(101).unwrap();
        assert_eq!(registry.cancel(&id, 102), JobCancelOutcome::Requested);

        reporter.succeed(DropProbe(dropped.clone()), 103).unwrap();

        assert!(dropped.load(Ordering::SeqCst));
        assert_eq!(registry.snapshot(&id).unwrap().state(), JobState::Cancelled);
        assert_eq!(
            registry.collect(&id, 104).unwrap_err(),
            JobCollectError::NotSucceeded
        );
    }

    // ---- Task 3: progress, notification, and diagnostics tests ----

    #[test]
    fn latest_progress_and_terminal_repair_coalesced_notifications() {
        let registry = LiveJobRegistry::<u32, String>::new();
        let watch = registry.watch().receiver();
        let (id, mut reporter) = registry
            .admit(direct_admission(7), no_op_control(), 100)
            .unwrap();
        reporter.mark_running(101).unwrap();
        reporter.report_progress(10, 102).unwrap();
        reporter.report_progress(20, 103).unwrap();
        reporter.succeed("seed".to_string(), 104).unwrap();

        assert!(watch.has_changed().unwrap());
        let snapshot = registry.snapshot(&id).unwrap();
        assert_eq!(snapshot.state(), JobState::Succeeded);
        assert_eq!(snapshot.progress(), Some(&20));
        assert_eq!(snapshot.revision(), 5);
    }

    #[test]
    fn diagnostics_keep_last_64_sanitized_entries_and_count_drops() {
        let registry = LiveJobRegistry::<u32, String>::new();
        let (id, mut reporter) = registry
            .admit(direct_admission(7), no_op_control(), 100)
            .unwrap();
        reporter.mark_running(101).unwrap();
        let entry = JobDiagnostic::new(
            JobDiagnosticCategory::Worker,
            "worker lifecycle observation",
        )
        .unwrap();
        for _ in 0..65 {
            reporter.append_diagnostic(entry.clone(), 102).unwrap();
        }

        let snapshot = registry.snapshot(&id).unwrap();
        assert_eq!(snapshot.diagnostics().len(), 64);
        assert_eq!(snapshot.dropped_diagnostics(), 1);
        const TOO_LONG: &str = concat!(
            "xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx",
            "xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx",
            "xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx",
            "xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx",
            "x"
        );
        assert_eq!(TOO_LONG.len(), 257);
        assert!(matches!(
            JobDiagnostic::new(JobDiagnosticCategory::Worker, TOO_LONG),
            Err(JobDiagnosticError::TooLong { limit: 256 })
        ));
    }

    #[test]
    fn snapshot_debug_omits_result_content_and_callback_markers() {
        let registry = LiveJobRegistry::<u32, String>::new();
        let (id, mut reporter) = registry
            .admit(direct_admission(7), no_op_control(), 100)
            .unwrap();
        reporter.mark_running(101).unwrap();
        reporter.succeed("SECRET_RESULT".to_string(), 102).unwrap();

        let snapshot = registry.snapshot(&id).unwrap();
        let debug_str = format!("{snapshot:?}");
        assert!(
            !debug_str.contains("SECRET_RESULT"),
            "Debug must not contain result content: {debug_str}"
        );
        assert!(
            !debug_str.contains("cancel_fn"),
            "Debug must not contain callback markers: {debug_str}"
        );
    }

    // ---- Task 3: collect-once, expiry, capacity, abandonment, shutdown tests ----

    #[test]
    fn success_result_moves_once_without_clone() {
        let registry = LiveJobRegistry::<u32, String>::new();
        let (id, mut reporter) = registry
            .admit(direct_admission(7), no_op_control(), 100)
            .unwrap();
        reporter.mark_running(101).unwrap();
        reporter.succeed("seed".to_string(), 102).unwrap();

        assert_eq!(registry.collect(&id, 103).unwrap(), "seed");
        assert_eq!(
            registry.collect(&id, 104).unwrap_err(),
            JobCollectError::AlreadyCollected
        );
        assert!(registry.snapshot(&id).unwrap().result_collected());
    }

    #[test]
    fn uncollected_result_expires_at_five_minutes() {
        let registry = LiveJobRegistry::<u32, DropProbe>::new();
        let dropped = Arc::new(AtomicBool::new(false));
        let (id, mut reporter) = registry
            .admit(direct_admission(7), no_op_control(), 0)
            .unwrap();
        reporter.mark_running(1).unwrap();
        reporter.succeed(DropProbe(dropped.clone()), 2).unwrap();

        registry.prune(2 + TERMINAL_TTL_MS);

        assert!(dropped.load(Ordering::SeqCst));
        assert_eq!(
            registry.collect(&id, 2 + TERMINAL_TTL_MS).unwrap_err(),
            JobCollectError::ResultExpired
        );
    }

    #[test]
    fn dropping_unfinished_reporter_marks_worker_abandoned() {
        let registry = LiveJobRegistry::<u32, String>::new();
        let (id, mut reporter) = registry
            .admit(direct_admission(7), no_op_control(), 100)
            .unwrap();
        reporter.mark_running(101).unwrap();
        drop(reporter);

        let snapshot = registry.snapshot(&id).unwrap();
        assert_eq!(snapshot.state(), JobState::Failed);
        assert_eq!(
            snapshot.failure_category(),
            Some(JobFailureCategory::WorkerAbandoned)
        );
    }

    #[test]
    fn shutdown_rejects_admission_and_requests_all_active_cancellation() {
        let calls = Arc::new(AtomicUsize::new(0));
        let registry = LiveJobRegistry::<u32, String>::new();
        let mut reporters = Vec::new();
        for nonce in 0..4 {
            let seen = calls.clone();
            reporters.push(
                registry
                    .admit(
                        direct_admission(nonce),
                        JobControl::new(move || {
                            seen.fetch_add(1, Ordering::SeqCst);
                        }),
                        nonce,
                    )
                    .unwrap()
                    .1,
            );
        }

        let requested = registry.shutdown(10);
        assert_eq!(requested.len(), 4);
        assert_eq!(calls.load(Ordering::SeqCst), 4);
        assert_eq!(
            registry
                .admit(direct_admission(9), no_op_control(), 11)
                .unwrap_err(),
            JobAdmissionError::ShuttingDown
        );
        drop(reporters);
    }

    // ---- Task 3: terminal-cap, result-slot, and cap tests ----

    #[test]
    fn terminal_cap_128_prunes_oldest_collected_on_next_admission() {
        let registry = LiveJobRegistry::<u32, String>::new();

        // Create and collect 128 terminal jobs.
        for i in 0..128u64 {
            let (id, mut reporter) = registry
                .admit(direct_admission(i), no_op_control(), i)
                .unwrap();
            reporter.mark_running(i + 1).unwrap();
            reporter.succeed(format!("result-{i}"), i + 2).unwrap();
            registry.collect(&id, i + 3).unwrap();
        }

        // All 128 terminal records still exist (collected but within TTL).
        let state = registry.inner.state.lock().unwrap();
        let terminal_count = state
            .jobs
            .values()
            .filter(|r| r.state.is_terminal())
            .count();
        assert_eq!(terminal_count, 128);
        drop(state);

        // Admission prunes the oldest collected record and succeeds.
        let (new_id, _reporter) = registry
            .admit(direct_admission(999), no_op_control(), 200)
            .unwrap();
        assert_eq!(
            registry.snapshot(&new_id).unwrap().state(),
            JobState::Starting
        );

        // The oldest collected record is gone.
        let state = registry.inner.state.lock().unwrap();
        let terminal_count = state
            .jobs
            .values()
            .filter(|r| r.state.is_terminal())
            .count();
        assert_eq!(terminal_count, 127);
    }

    #[test]
    fn active_plus_uncollected_success_reserves_four_result_slots() {
        let registry = LiveJobRegistry::<u32, String>::new();

        // Fill all 4 slots: 2 active + 2 uncollected successes.
        let (_id1, _r1) = registry
            .admit(direct_admission(0), no_op_control(), 0)
            .unwrap();
        let (_id2, _r2) = registry
            .admit(direct_admission(1), no_op_control(), 1)
            .unwrap();
        let (id3, mut r3) = registry
            .admit(direct_admission(2), no_op_control(), 2)
            .unwrap();
        r3.mark_running(3).unwrap();
        r3.succeed("res3".to_string(), 4).unwrap();
        let (id4, mut r4) = registry
            .admit(direct_admission(3), no_op_control(), 3)
            .unwrap();
        r4.mark_running(4).unwrap();
        r4.succeed("res4".to_string(), 5).unwrap();

        // 5th admission fails with ResultCapacity.
        assert_eq!(
            registry
                .admit(direct_admission(5), no_op_control(), 5)
                .unwrap_err(),
            JobAdmissionError::ResultCapacity { limit: 4 }
        );

        // Collect one result to free a slot.
        registry.collect(&id3, 6).unwrap();

        // Now the same admission succeeds.
        let (id5, _r5) = registry
            .admit(direct_admission(6), no_op_control(), 7)
            .unwrap();
        assert_eq!(registry.snapshot(&id5).unwrap().state(), JobState::Starting);

        // Collect the other result.
        registry.collect(&id4, 8).unwrap();
    }

    #[test]
    fn uncollected_unexpired_terminal_records_are_not_silently_evicted() {
        let registry = LiveJobRegistry::<u32, String>::new();

        // Create 128 uncollected terminal records (failures, no result to collect).
        for i in 0..128u64 {
            let (_id, mut reporter) = registry
                .admit(direct_admission(i), no_op_control(), i)
                .unwrap();
            reporter.mark_running(i + 1).unwrap();
            reporter
                .fail(JobFailureCategory::WorkerAbandoned, i + 2)
                .unwrap();
        }

        // All 128 still exist (within TTL).
        let state = registry.inner.state.lock().unwrap();
        let terminal_count = state
            .jobs
            .values()
            .filter(|r| r.state.is_terminal())
            .count();
        assert_eq!(terminal_count, 128);
        drop(state);

        // Prune after TTL — records persist until cap eviction.
        registry.prune(129 + TERMINAL_TTL_MS);

        let state = registry.inner.state.lock().unwrap();
        let terminal_count = state
            .jobs
            .values()
            .filter(|r| r.state.is_terminal())
            .count();
        assert_eq!(terminal_count, 128);
        drop(state);

        // Cap eviction: admitting a new job prunes the oldest terminal.
        let (_new_id, _reporter) = registry
            .admit(
                direct_admission(999),
                no_op_control(),
                129 + TERMINAL_TTL_MS,
            )
            .unwrap();

        let state = registry.inner.state.lock().unwrap();
        let terminal_count = state
            .jobs
            .values()
            .filter(|r| r.state.is_terminal())
            .count();
        assert_eq!(terminal_count, 127);
    }

    #[test]
    fn ttl_expired_result_is_tombstoned_for_collect_after_eviction() {
        let registry = LiveJobRegistry::<u32, String>::new();
        let (id, mut reporter) = registry
            .admit(direct_admission(7), no_op_control(), 0)
            .unwrap();
        reporter.mark_running(1).unwrap();
        reporter.succeed("result".to_string(), 2).unwrap();

        // Prune at TTL: result dropped, tombstoned.
        registry.prune(2 + TERMINAL_TTL_MS);

        // Verify tombstone exists.
        let state = registry.inner.state.lock().unwrap();
        assert!(
            state.tombstones.contains(id.as_str()),
            "should be tombstoned"
        );
        drop(state);

        // Record still exists in registry.
        assert!(registry.snapshot(&id).is_some());

        // collect returns ResultExpired via the result-expired path.
        assert_eq!(
            registry.collect(&id, 2 + TERMINAL_TTL_MS).unwrap_err(),
            JobCollectError::ResultExpired
        );

        // Now manually evict the record and verify the tombstone still works.
        {
            let mut state = registry.inner.state.lock().unwrap();
            state.jobs.remove(&id);
        }

        // collect returns ResultExpired via the tombstone path (record evicted).
        assert_eq!(
            registry.collect(&id, 3 + TERMINAL_TTL_MS).unwrap_err(),
            JobCollectError::ResultExpired
        );
    }

    #[test]
    fn cancel_not_found_returns_not_found() {
        let registry = LiveJobRegistry::<u32, String>::new();
        let fake_id = JobId::parse("job-00000000-0000-4000-8000-000000000099").unwrap();
        assert_eq!(registry.cancel(&fake_id, 100), JobCancelOutcome::NotFound);
    }

    #[test]
    fn dropping_starting_reporter_marks_worker_abandoned() {
        let registry = LiveJobRegistry::<u32, String>::new();
        let (id, reporter) = registry
            .admit(direct_admission(7), no_op_control(), 100)
            .unwrap();
        // Never mark_running — still Starting.
        drop(reporter);

        let snapshot = registry.snapshot(&id).unwrap();
        assert_eq!(snapshot.state(), JobState::Failed);
        assert_eq!(
            snapshot.failure_category(),
            Some(JobFailureCategory::WorkerAbandoned)
        );
    }

    #[test]
    fn dropping_reporter_while_cancelling_confirms_cancellation() {
        let calls = Arc::new(AtomicUsize::new(0));
        let seen = calls.clone();
        let registry = LiveJobRegistry::<u32, String>::new();
        let (id, reporter) = registry
            .admit(
                direct_admission(7),
                JobControl::new(move || {
                    seen.fetch_add(1, Ordering::SeqCst);
                }),
                100,
            )
            .unwrap();
        // Cancel from Starting.
        assert_eq!(registry.cancel(&id, 101), JobCancelOutcome::Requested);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert_eq!(
            registry.snapshot(&id).unwrap().state(),
            JobState::Cancelling
        );

        // Drop reporter — should confirm cancellation.
        drop(reporter);

        assert_eq!(registry.snapshot(&id).unwrap().state(), JobState::Cancelled);
    }

    #[test]
    fn terminal_records_track_terminal_time_for_ttl() {
        let registry = LiveJobRegistry::<u32, String>::new();
        let (id, mut reporter) = registry
            .admit(direct_admission(7), no_op_control(), 0)
            .unwrap();
        reporter.mark_running(100).unwrap();
        reporter
            .fail(JobFailureCategory::WorkerAbandoned, 200)
            .unwrap();

        let snapshot = registry.snapshot(&id).unwrap();
        assert_eq!(snapshot.terminal_at_ms(), Some(200));
        assert_eq!(snapshot.created_at_ms(), 0);

        // Prune just before TTL from terminal time — record persists.
        registry.prune(200 + TERMINAL_TTL_MS - 1);
        assert!(registry.snapshot(&id).is_some());

        // Prune at TTL from terminal time — record persists until cap eviction.
        registry.prune(200 + TERMINAL_TTL_MS);
        assert!(registry.snapshot(&id).is_some());
    }

    // ---- Task 5: owner-drop and privacy tests ----

    #[test]
    fn owner_drop_requests_cancel_while_observer_and_reporter_finish_cleanup() {
        let calls = Arc::new(AtomicUsize::new(0));
        let seen = calls.clone();
        let registry = LiveJobRegistry::<u32, String>::new();
        let observer = registry.observer();
        let (id, mut reporter) = registry
            .admit(
                direct_admission(7),
                JobControl::new(move || {
                    seen.fetch_add(1, Ordering::SeqCst);
                }),
                10,
            )
            .unwrap();
        reporter.mark_running(11).unwrap();

        drop(registry);

        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert_eq!(observer.snapshot(&id).unwrap().state(), JobState::Cancelling);
        reporter.cancelled(12).unwrap();
        assert_eq!(observer.snapshot(&id).unwrap().state(), JobState::Cancelled);
    }

    /// Capture tracing output for the duration of `run`.
    ///
    /// Registers a capturing tracing subscriber and runs `run` inside its
    /// scope. Uses `set_global_default` (ignoring the "already set" error
    /// when a prior test registered first) and falls back to `set_default`.
    fn capture_job_tracing<T>(run: impl FnOnce() -> T) -> (T, String) {
        use tracing_subscriber::layer::SubscriberExt;
        use tracing_subscriber::Layer;
        use tracing_subscriber::Registry;

        let log_buffer: Arc<std::sync::Mutex<Vec<u8>>> =
            Arc::new(std::sync::Mutex::new(Vec::new()));
        let log_check = log_buffer.clone();

        let make_subscriber =
            |buf: Arc<std::sync::Mutex<Vec<u8>>>| {
                let buf2 = buf.clone();
                let fmt_layer = tracing_subscriber::fmt::layer()
                    .with_writer(move || WriteAdaptor { buf: buf2.clone() })
                    .with_ansi(false)
                    .with_target(true)
                    .without_time()
                    .with_filter(tracing::level_filters::LevelFilter::TRACE);
                Registry::default().with(fmt_layer)
            };

        // Try to register as global first. On failure (already set),
        // use a thread-local subscriber so this test's events still
        // route to our capturing writer.
        let _local_guard = match tracing::subscriber::set_global_default(
            make_subscriber(log_buffer),
        ) {
            Ok(()) => None,
            Err(_) => Some(tracing::subscriber::set_default(
                make_subscriber(log_check.clone()),
            )),
        };

        let result = run();
        let logs = String::from_utf8(log_check.lock().unwrap().to_vec()).unwrap();
        (result, logs)
    }

    /// Adapts `Arc<Mutex<Vec<u8>>>` to `std::io::Write` for tracing subscriber.
    struct WriteAdaptor {
        buf: Arc<std::sync::Mutex<Vec<u8>>>,
    }

    impl std::io::Write for WriteAdaptor {
        fn write(&mut self, data: &[u8]) -> std::io::Result<usize> {
            self.buf.lock().unwrap().write(data)
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn job_debug_and_tracing_omit_control_and_result_sentinels() {
        let sentinels = [
            "/home/alice/SECRET-recording.mp4",
            "RAW-FFMPEG-SECRET",
            "api_key=SECRET",
            "SECRET-skill-body",
            "SECRET-seed-payload",
        ];
        let captured_by_control = sentinels.join("|");
        let result_payload = sentinels.join("|");
        let ((registry, id), logs) = capture_job_tracing(move || {
            let registry = LiveJobRegistry::<u32, String>::new();
            let control_secret = captured_by_control;
            let (id, mut reporter) = registry
                .admit(
                    direct_admission(7),
                    JobControl::new(move || {
                        std::hint::black_box(&control_secret);
                    }),
                    10,
                )
                .unwrap();
            reporter.mark_running(11).unwrap();
            reporter.report_progress(25, 12).unwrap();
            reporter.succeed(result_payload, 13).unwrap();
            (registry, id)
        });
        let rendered = format!(
            "{:?}{:?}{:?}",
            registry.snapshot(&id).unwrap(),
            registry.watch(),
            logs
        );

        for sentinel in sentinels {
            assert!(!rendered.contains(sentinel), "leaked sentinel: {sentinel}");
        }
        assert!(
            logs.contains("rollshot::agent::jobs"),
            "logs must contain target, got: {logs:?}"
        );
        assert!(
            logs.contains(id.as_str()),
            "logs must contain job id, got: {logs:?}"
        );
        assert!(
            logs.contains("Succeeded"),
            "logs must contain 'Succeeded', got: {logs:?}"
        );
    }
}
