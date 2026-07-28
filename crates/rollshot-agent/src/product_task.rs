//! Product Task, artifact promotion, and canonical V1 digest contracts.
//!
//! Framework-neutral: no iced, no filesystem, no Tokio. All fields private;
//! read-only accessors expose required values. Pure reducers produce new
//! snapshots by cloning internal state, validating the transition, and
//! incrementing `snapshot_revision` exactly once.

use std::collections::BTreeMap;
use std::fmt;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::authority::AuthoritySnapshotReceiptV1;
use crate::domain::RunId;
use crate::skills::SkillUseReceiptV1;

// ========================================================================
// Opaque IDs
// ========================================================================

/// One user-authorized Smart Redaction request.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ProductTaskId(String);

impl ProductTaskId {
    pub fn parse(value: impl Into<String>) -> Result<Self, String> {
        let value = value.into();
        if valid_uuid_suffix(&value, "task-") {
            Ok(Self(value))
        } else {
            Err(format!("invalid ProductTaskId: {value}"))
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Serialize for ProductTaskId {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for ProductTaskId {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        Self::parse(s).map_err(serde::de::Error::custom)
    }
}

/// Logical promoted review artifact.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ArtifactId(String);

impl ArtifactId {
    pub fn parse(value: impl Into<String>) -> Result<Self, String> {
        let value = value.into();
        if valid_uuid_suffix(&value, "artifact-") {
            Ok(Self(value))
        } else {
            Err(format!("invalid ArtifactId: {value}"))
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Serialize for ArtifactId {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for ArtifactId {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        Self::parse(s).map_err(serde::de::Error::custom)
    }
}

/// Immutable payload revision reviewed by the user.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ArtifactRevision(u32);

impl ArtifactRevision {
    pub fn new(raw: u32) -> Self {
        Self(raw)
    }

    pub fn get(self) -> u32 {
        self.0
    }
}

/// One bounded execution attempt inside a Product Task.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct TaskAttemptId(u32);

impl fmt::Display for TaskAttemptId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl TaskAttemptId {
    pub fn new(raw: u32) -> Self {
        Self(raw)
    }

    pub fn get(self) -> u32 {
        self.0
    }
}

// ========================================================================
// Enums
// ========================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskKind {
    SmartRedactionAuthor,
    SmartRedactionImprove,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    Created,
    Running,
    ReadyForReview,
    Applying,
    Completed,
    Rejected,
    Stale,
    #[serde(rename_all = "snake_case")]
    Failed {
        terminal: TaskTerminal,
    },
    NeedsUserInput,
    Cancelled,
    Interrupted,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskTerminal {
    NeedsUserInput,
    Cancelled,
    BudgetExhausted { dimension: String },
    SourceValidationFailure,
    RuntimeFailure,
    AgentProtocolFailure,
    ProviderFailure,
    Interrupted,
    Stale,
    ContextOverflow,
    ContextRecoveryFailure { category: String },
    AuditFailure { category: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactKind {
    SmartRedaction,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PayloadMode {
    Author,
    Improve,
}

// ========================================================================
// Source binding
// ========================================================================

/// Domain-tagged binding identifying the source a task acts on.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "domain", rename_all = "snake_case")]
pub enum SourceBinding {
    SmartRedaction {
        base_image_sha256: [u8; 32],
        annotation_state_sha256: [u8; 32],
        document_state_id: u32,
        preset_id: String,
        active_preset_revision_id: Option<String>,
    },
    ActionGuideProject {
        /// SHA-256 of the canonicalized project root path. The project manifest
        /// has no stable identity, so the path is the only one available.
        project_root_sha256: [u8; 32],
        revision: u64,
        projection_digest: String,
    },
    ActionGuideEphemeralGuide {
        guide_digest: String,
    },
}

impl SourceBinding {
    /// Constructor preserving the pre-migration argument order.
    pub fn smart_redaction(
        base_image_sha256: [u8; 32],
        annotation_state_sha256: [u8; 32],
        document_state_id: u32,
        preset_id: String,
        active_preset_revision_id: Option<String>,
    ) -> Self {
        Self::SmartRedaction {
            base_image_sha256,
            annotation_state_sha256,
            document_state_id,
            preset_id,
            active_preset_revision_id,
        }
    }

    /// Smart Redaction base-image digest, or `None` for other domains.
    pub fn smart_redaction_base_image_sha256(&self) -> Option<&[u8; 32]> {
        match self {
            Self::SmartRedaction {
                base_image_sha256, ..
            } => Some(base_image_sha256),
            _ => None,
        }
    }
}

// ========================================================================
// Task attempt
// ========================================================================

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskAttempt {
    attempt_id: TaskAttemptId,
    run_id: RunId,
    started_at_unix_ms: i64,
    finished_at_unix_ms: Option<i64>,
    terminal: Option<TaskTerminal>,
    /// V2 provenance binding. `None` for V1 tasks or before `bind_run_contract`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    run_contract: Option<RunContractReceiptV1>,
}

impl TaskAttempt {
    pub fn new(attempt_id: TaskAttemptId, run_id: RunId, started_at_unix_ms: i64) -> Self {
        Self {
            attempt_id,
            run_id,
            started_at_unix_ms,
            finished_at_unix_ms: None,
            terminal: None,
            run_contract: None,
        }
    }

    pub fn attempt_id(&self) -> TaskAttemptId {
        self.attempt_id
    }

    pub fn run_id(&self) -> &RunId {
        &self.run_id
    }

    pub fn started_at_unix_ms(&self) -> i64 {
        self.started_at_unix_ms
    }

    pub fn finished_at_unix_ms(&self) -> Option<i64> {
        self.finished_at_unix_ms
    }

    pub fn terminal(&self) -> Option<TaskTerminal> {
        self.terminal.clone()
    }

    pub fn run_contract(&self) -> Option<&RunContractReceiptV1> {
        self.run_contract.as_ref()
    }
}

// ========================================================================
// Artifact metadata
// ========================================================================

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProductArtifactMetadata {
    artifact_id: ArtifactId,
    artifact_revision: ArtifactRevision,
    kind: ArtifactKind,
    schema_version: u32,
    canonical_payload_sha256: String,
    source_binding: SourceBinding,
    task_id: ProductTaskId,
    attempt_id: TaskAttemptId,
    run_id: RunId,
    proposal_id: String,
    provider_id: String,
    model_id: String,
    run_config_digest: String,
    dry_run_candidate_count: u32,
    dry_run_affected_area: f32,
    created_at_unix_ms: i64,
    /// V2 provenance. `None` for V1 artifacts.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    run_contract: Option<RunContractReceiptV1>,
}

impl ProductArtifactMetadata {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        artifact_id: ArtifactId,
        artifact_revision: ArtifactRevision,
        kind: ArtifactKind,
        schema_version: u32,
        canonical_payload_sha256: String,
        source_binding: SourceBinding,
        task_id: ProductTaskId,
        attempt_id: TaskAttemptId,
        run_id: RunId,
        proposal_id: String,
        provider_id: String,
        model_id: String,
        run_config_digest: String,
        dry_run_candidate_count: u32,
        dry_run_affected_area: f32,
        created_at_unix_ms: i64,
    ) -> Self {
        Self {
            artifact_id,
            artifact_revision,
            kind,
            schema_version,
            canonical_payload_sha256,
            source_binding,
            task_id,
            attempt_id,
            run_id,
            proposal_id,
            provider_id,
            model_id,
            run_config_digest,
            dry_run_candidate_count,
            dry_run_affected_area,
            created_at_unix_ms,
            run_contract: None,
        }
    }

    /// V2 constructor: includes run-contract provenance.
    #[allow(clippy::too_many_arguments)]
    pub fn new_v2(
        artifact_id: ArtifactId,
        artifact_revision: ArtifactRevision,
        kind: ArtifactKind,
        schema_version: u32,
        canonical_payload_sha256: String,
        source_binding: SourceBinding,
        task_id: ProductTaskId,
        attempt_id: TaskAttemptId,
        run_id: RunId,
        proposal_id: String,
        provider_id: String,
        model_id: String,
        run_config_digest: String,
        dry_run_candidate_count: u32,
        dry_run_affected_area: f32,
        created_at_unix_ms: i64,
        run_contract: RunContractReceiptV1,
    ) -> Self {
        Self {
            artifact_id,
            artifact_revision,
            kind,
            schema_version,
            canonical_payload_sha256,
            source_binding,
            task_id,
            attempt_id,
            run_id,
            proposal_id,
            provider_id,
            model_id,
            run_config_digest,
            dry_run_candidate_count,
            dry_run_affected_area,
            created_at_unix_ms,
            run_contract: Some(run_contract),
        }
    }

    pub fn artifact_id(&self) -> &ArtifactId {
        &self.artifact_id
    }

    pub fn artifact_revision(&self) -> ArtifactRevision {
        self.artifact_revision
    }

    pub fn task_id(&self) -> &ProductTaskId {
        &self.task_id
    }

    pub fn run_id(&self) -> &RunId {
        &self.run_id
    }

    pub fn proposal_id(&self) -> &str {
        &self.proposal_id
    }

    pub fn source_binding(&self) -> &SourceBinding {
        &self.source_binding
    }

    pub fn canonical_payload_sha256(&self) -> &str {
        &self.canonical_payload_sha256
    }

    pub fn run_config_digest(&self) -> &str {
        &self.run_config_digest
    }

    pub fn run_contract(&self) -> Option<&RunContractReceiptV1> {
        self.run_contract.as_ref()
    }

    pub fn kind(&self) -> ArtifactKind {
        self.kind
    }

    pub fn schema_version(&self) -> u32 {
        self.schema_version
    }

    pub fn attempt_id(&self) -> TaskAttemptId {
        self.attempt_id
    }

    pub fn provider_id(&self) -> &str {
        &self.provider_id
    }

    pub fn model_id(&self) -> &str {
        &self.model_id
    }
}

// ========================================================================
// Smart Redaction review payload (privacy-bounded)
// ========================================================================

/// Privacy-bounded payload — excludes pixels, text, transcripts,
/// credentials, raw OCR, provider-native values.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SmartRedactionReviewPayload {
    pub source: PayloadSourceV1,
    pub proposal: PayloadProposalV1,
    pub dry_run: PayloadDryRunV1,
    pub config: PayloadConfigV1,
}

/// Canonical V1 source DTO — ordered fields, no secrets.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PayloadSourceV1 {
    pub kind: String,
    pub validation_summary: String,
}

/// Canonical V1 proposal DTO — bounded geometry, no raw bytes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PayloadProposalV1 {
    pub proposal_id: String,
    pub candidate_count: u32,
}

/// Canonical V1 dry-run DTO — bounded scalars.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PayloadDryRunV1 {
    pub candidate_count: u32,
    pub affected_area: f32,
}

/// Canonical V1 config DTO — privacy-filtered, no secrets/keys/paths.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PayloadConfigV1 {
    pub provider: String,
    pub model: String,
    pub payload_mode: PayloadMode,
    pub run_kind: String,
    pub budget_dimensions: BTreeMap<String, u64>,
}

// ========================================================================
// Canonical V1 annotation-state DTO
// ========================================================================

/// DTO for canonical annotation-state digest. Contains image dimensions,
/// ordered annotations, and document state ID. BTreeMap ensures stable ordering.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AnnotationStateV1 {
    pub width: u32,
    pub height: u32,
    pub state_id: u32,
    pub annotations: Vec<AnnotationV1>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AnnotationV1 {
    pub kind: String,
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

// ========================================================================
// Run config fingerprint (privacy-filtered — no secrets/keys/paths)
// ========================================================================

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RunConfigFingerprintV1 {
    pub provider: String,
    pub model: String,
    pub payload_mode: PayloadMode,
    pub run_kind: String,
    pub budget_dimensions: BTreeMap<String, u64>,
}

// ========================================================================
// Content binding
// ========================================================================

/// Cached content binding: base-image digest + annotation-state digest + state ID.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocumentContentBinding {
    base_image_digest: [u8; 32],
    annotation_state_digest: [u8; 32],
    state_id: u32,
}

impl DocumentContentBinding {
    pub fn new(
        base_image_digest: [u8; 32],
        annotation_state: &AnnotationStateV1,
        state_id: u32,
    ) -> Result<Self, CanonicalError> {
        let annotation_state_digest = compute_annotation_state_digest(annotation_state)?;
        Ok(Self {
            base_image_digest,
            annotation_state_digest,
            state_id,
        })
    }

    pub fn base_image_digest(&self) -> &[u8; 32] {
        &self.base_image_digest
    }

    pub fn annotation_state_digest(&self) -> &[u8; 32] {
        &self.annotation_state_digest
    }

    pub fn state_id(&self) -> u32 {
        self.state_id
    }
}

// ========================================================================
// Local review delta and receipt
// ========================================================================

/// Validated local modifications to artifact candidates + manual additions.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LocalReviewDeltaV1 {
    pub moved_candidates: Vec<(u32, u32)>,
    pub manual_additions: Vec<ManualCandidateV1>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ManualCandidateV1 {
    pub local_id: u32,
    pub kind: String,
}

/// Review receipt — binds exact artifact revision and actual post-apply state.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReviewReceipt {
    pub artifact_id: ArtifactId,
    pub artifact_revision: ArtifactRevision,
    pub proposal_id: String,
    pub applied_candidates: Vec<u32>,
    pub rejected_candidates: Vec<u32>,
    pub local_delta: LocalReviewDeltaV1,
    pub resulting_document_state_id: Option<u32>,
    pub resulting_document_digest: Option<[u8; 32]>,
    pub decided_at_unix_ms: i64,
}

// ========================================================================
// Promotion context (app-supplied values for artifact creation)
// ========================================================================

#[derive(Debug, Clone)]
pub struct PromotionContext {
    pub artifact_id: ArtifactId,
    pub task_id: ProductTaskId,
    pub attempt_id: TaskAttemptId,
    pub run_id: RunId,
    pub proposal_id: String,
    pub provider_id: String,
    pub model_id: String,
    pub run_config_digest: String,
    pub source: PayloadSourceV1,
    pub proposal: PayloadProposalV1,
}

// ========================================================================
// Run contract receipt (V2 provenance binding)
// ========================================================================

/// Binds authority snapshot + skill-use receipts to a task attempt.
/// Created by `ProductTaskSnapshot::bind_run_contract` while `Running`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunContractReceiptV1 {
    pub authority: AuthoritySnapshotReceiptV1,
    pub skill_use: SkillUseReceiptV1,
    pub bound_at_unix_ms: i64,
}

// ========================================================================
// Run config fingerprint V2 (privacy-filtered, includes provenance)
// ========================================================================

/// V2 fingerprint: extends V1 with authority snapshot digest and exact
/// skill-use receipt. Uses a single `skill_use` field; supporting
/// multiple invoked skills is deferred until a real workload requires it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RunConfigFingerprintV2 {
    pub provider: String,
    pub model: String,
    pub payload_mode: PayloadMode,
    pub run_kind: String,
    pub budget_dimensions: BTreeMap<String, u64>,
    pub authority_snapshot_digest: String,
    pub skill_use: SkillUseReceiptV1,
}

impl ValidateFinite for SourceBinding {
    fn check_finite(&self) -> Result<(), CanonicalError> {
        Ok(())
    }
}

impl ValidateFinite for RunConfigFingerprintV2 {
    fn check_finite(&self) -> Result<(), CanonicalError> {
        Ok(())
    }
}

// ========================================================================
// Errors
// ========================================================================

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TaskContractError {
    #[error("illegal transition from {from:?} to attempted action")]
    IllegalTransition { from: TaskStatus },
    #[error("timestamp regression: current {current}, attempted {attempted}")]
    TimestampRegression { current: i64, attempted: i64 },
    #[error("missing attempt")]
    MissingAttempt,
    #[error("conflicting attempt: expected {expected}, got {got}")]
    ConflictingAttempt {
        expected: TaskAttemptId,
        got: TaskAttemptId,
    },
    #[error("unsupported terminal for record_terminal: {terminal:?}; use mark_stale or reconcile_interrupted instead")]
    UnsupportedTerminal { terminal: TaskTerminal },
    #[error("run ID mismatch: expected {expected}, got {got}")]
    RunMismatch { expected: String, got: String },
    #[error("proposal ID mismatch: expected {expected}, got {got}")]
    ProposalMismatch { expected: String, got: String },
    #[error("artifact ID mismatch: expected {expected}, got {got}")]
    ArtifactMismatch { expected: String, got: String },
    #[error("artifact revision mismatch: expected {expected}, got {got}")]
    RevisionMismatch { expected: u32, got: u32 },
    #[error("missing payload or metadata")]
    MissingPayload,
    #[error("run contract already bound with a different receipt")]
    RunContractConflict,
    #[error("run contract required for schema version 2")]
    MissingRunContract,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CanonicalError {
    #[error("non-finite float value")]
    NonFiniteFloat,
    #[error("string exceeds max length: {len} > {max}")]
    StringTooLong { len: usize, max: usize },
    #[error("collection exceeds max size: {len} > {max}")]
    CollectionTooLarge { len: usize, max: usize },
    #[error("serialization failed: {0}")]
    Serialization(String),
}

// ========================================================================
// Product Task snapshot (all fields private)
// ========================================================================

const SCHEMA_VERSION: u32 = 1;
const MAX_CANONICAL_STRING_LEN: usize = 4096;
const MAX_CANONICAL_COLLECTION_LEN: usize = 256;

#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub struct ProductTaskSnapshot {
    store_schema_version: u32,
    snapshot_revision: u32,
    task_id: ProductTaskId,
    kind: TaskKind,
    source_binding: SourceBinding,
    status: TaskStatus,
    attempts: Vec<TaskAttempt>,
    artifact_metadata: Option<ProductArtifactMetadata>,
    pending_artifact_payload: Option<Vec<u8>>,
    /// Serialized `EditProposal` JSON, stored at ReadyForReview so the
    /// workbench can restore the full proposal from the store without a
    /// provider call.  `#[serde(default)]` for backward compat.
    #[serde(default)]
    pending_proposal_payload: Option<Vec<u8>>,
    review_receipt: Option<ReviewReceipt>,
    created_at_unix_ms: i64,
    updated_at_unix_ms: i64,
}

impl ProductTaskSnapshot {
    /// Create a new task in `Created` status with `snapshot_revision = 0`.
    pub fn new(
        task_id: ProductTaskId,
        kind: TaskKind,
        source_binding: SourceBinding,
        now: i64,
    ) -> Result<Self, TaskContractError> {
        Ok(Self {
            store_schema_version: SCHEMA_VERSION,
            snapshot_revision: 0,
            task_id,
            kind,
            source_binding,
            status: TaskStatus::Created,
            attempts: Vec::new(),
            artifact_metadata: None,
            pending_artifact_payload: None,
            pending_proposal_payload: None,
            review_receipt: None,
            created_at_unix_ms: now,
            updated_at_unix_ms: now,
        })
    }

    // -- Read-only accessors --

    pub fn store_schema_version(&self) -> u32 {
        self.store_schema_version
    }

    pub fn snapshot_revision(&self) -> u32 {
        self.snapshot_revision
    }

    pub fn task_id(&self) -> &ProductTaskId {
        &self.task_id
    }

    pub fn kind(&self) -> TaskKind {
        self.kind
    }

    pub fn source_binding(&self) -> &SourceBinding {
        &self.source_binding
    }

    pub fn status(&self) -> TaskStatus {
        self.status.clone()
    }

    pub fn attempts(&self) -> &[TaskAttempt] {
        &self.attempts
    }

    pub fn artifact_metadata(&self) -> Option<&ProductArtifactMetadata> {
        self.artifact_metadata.as_ref()
    }

    pub fn pending_artifact_payload(&self) -> Option<&[u8]> {
        self.pending_artifact_payload.as_deref()
    }

    pub fn pending_proposal_payload(&self) -> Option<&[u8]> {
        self.pending_proposal_payload.as_deref()
    }

    pub fn review_receipt(&self) -> Option<&ReviewReceipt> {
        self.review_receipt.as_ref()
    }

    pub fn created_at_unix_ms(&self) -> i64 {
        self.created_at_unix_ms
    }

    pub fn updated_at_unix_ms(&self) -> i64 {
        self.updated_at_unix_ms
    }

    // -- Reducers --

    /// Transition: Created → Running. Adds the attempt and increments revision.
    pub fn start_attempt(&self, attempt: TaskAttempt, now: i64) -> Result<Self, TaskContractError> {
        if self.status != TaskStatus::Created {
            return Err(TaskContractError::IllegalTransition {
                from: self.status.clone(),
            });
        }
        if now < self.updated_at_unix_ms {
            return Err(TaskContractError::TimestampRegression {
                current: self.updated_at_unix_ms,
                attempted: now,
            });
        }
        let mut next = self.clone();
        next.status = TaskStatus::Running;
        next.attempts.push(attempt);
        next.snapshot_revision += 1;
        next.updated_at_unix_ms = now;
        Ok(next)
    }

    /// V2 constructor: creates a task with schema version 2.
    /// V2 tasks require a run-contract binding before promotion.
    pub fn new_v2(
        task_id: ProductTaskId,
        kind: TaskKind,
        source_binding: SourceBinding,
        now: i64,
    ) -> Result<Self, TaskContractError> {
        Ok(Self {
            store_schema_version: 2,
            snapshot_revision: 0,
            task_id,
            kind,
            source_binding,
            status: TaskStatus::Created,
            attempts: Vec::new(),
            artifact_metadata: None,
            pending_artifact_payload: None,
            pending_proposal_payload: None,
            review_receipt: None,
            created_at_unix_ms: now,
            updated_at_unix_ms: now,
        })
    }

    /// Bind a run-contract receipt to the active `Running` attempt.
    ///
    /// Rules:
    /// - Only valid while `Running`.
    /// - Receipt task_id, attempt_id, run_id must match the active attempt.
    /// - Timestamp must be monotonically non-decreasing.
    /// - Attempt must not be terminal (finished).
    /// - Missing receipt binds and increments `snapshot_revision` once.
    /// - Byte-for-byte identical receipt is idempotent (no revision bump).
    /// - Any different existing receipt is `RunContractConflict`.
    pub fn bind_run_contract(
        &self,
        receipt: RunContractReceiptV1,
        now: i64,
    ) -> Result<Self, TaskContractError> {
        if self.status != TaskStatus::Running {
            return Err(TaskContractError::IllegalTransition {
                from: self.status.clone(),
            });
        }
        if now < self.updated_at_unix_ms {
            return Err(TaskContractError::TimestampRegression {
                current: self.updated_at_unix_ms,
                attempted: now,
            });
        }
        let last_attempt = self
            .attempts
            .last()
            .ok_or(TaskContractError::MissingAttempt)?;
        // Receipt must not target a terminal attempt.
        if last_attempt.terminal.is_some() {
            return Err(TaskContractError::IllegalTransition {
                from: self.status.clone(),
            });
        }
        // Receipt run_id must match the active attempt.
        if receipt.authority.run_id != last_attempt.run_id.as_str() {
            return Err(TaskContractError::RunMismatch {
                expected: last_attempt.run_id.as_str().to_owned(),
                got: receipt.authority.run_id.clone(),
            });
        }
        // Receipt task_id must match the snapshot task.
        if receipt.authority.task_id != self.task_id.as_str() {
            return Err(TaskContractError::RunMismatch {
                expected: self.task_id.as_str().to_owned(),
                got: receipt.authority.task_id.clone(),
            });
        }
        // Receipt attempt_id must match the active attempt.
        if receipt.authority.attempt_id != last_attempt.attempt_id.get() {
            return Err(TaskContractError::ConflictingAttempt {
                expected: last_attempt.attempt_id,
                got: TaskAttemptId::new(receipt.authority.attempt_id),
            });
        }
        // Idempotent check: identical receipt → no change.
        if let Some(existing) = &last_attempt.run_contract {
            if *existing == receipt {
                return Ok(self.clone());
            }
            return Err(TaskContractError::RunContractConflict);
        }
        let mut next = self.clone();
        if let Some(attempt) = next.attempts.last_mut() {
            attempt.run_contract = Some(receipt);
        }
        next.snapshot_revision += 1;
        next.updated_at_unix_ms = now;
        Ok(next)
    }

    /// The run-contract receipt on the active attempt, if any.
    pub fn active_run_contract(&self) -> Option<&RunContractReceiptV1> {
        self.attempts.last()?.run_contract.as_ref()
    }

    /// Transition: Running → ReadyForReview with canonical review
    /// payload, and optionally the serialized EditProposal for restore.
    /// Increments `snapshot_revision` exactly once.
    pub fn record_ready_for_review(
        &self,
        metadata: ProductArtifactMetadata,
        payload: SmartRedactionReviewPayload,
        proposal_payload: Option<Vec<u8>>,
        now: i64,
    ) -> Result<Self, TaskContractError> {
        if self.status != TaskStatus::Running {
            return Err(TaskContractError::IllegalTransition {
                from: self.status.clone(),
            });
        }
        if now < self.updated_at_unix_ms {
            return Err(TaskContractError::TimestampRegression {
                current: self.updated_at_unix_ms,
                attempted: now,
            });
        }
        let last_attempt = self
            .attempts
            .last()
            .ok_or(TaskContractError::MissingAttempt)?;
        if metadata.attempt_id != last_attempt.attempt_id {
            return Err(TaskContractError::ConflictingAttempt {
                expected: last_attempt.attempt_id,
                got: metadata.attempt_id,
            });
        }
        if *metadata.run_id() != last_attempt.run_id {
            return Err(TaskContractError::RunMismatch {
                expected: last_attempt.run_id.as_str().to_owned(),
                got: metadata.run_id().as_str().to_owned(),
            });
        }
        // V2 schema requires an active run contract on the attempt.
        if self.store_schema_version >= 2 && last_attempt.run_contract.is_none() {
            return Err(TaskContractError::MissingRunContract);
        }
        let payload_bytes =
            serde_json::to_vec(&payload).map_err(|_e| TaskContractError::MissingPayload)?;
        let mut next = self.clone();
        next.status = TaskStatus::ReadyForReview;
        next.artifact_metadata = Some(metadata);
        next.pending_artifact_payload = Some(payload_bytes);
        next.pending_proposal_payload = proposal_payload;
        next.snapshot_revision += 1;
        next.updated_at_unix_ms = now;
        Ok(next)
    }

    /// Transition: Running → terminal Failed status.
    pub fn record_terminal(
        &self,
        terminal: TaskTerminal,
        now: i64,
    ) -> Result<Self, TaskContractError> {
        if self.status != TaskStatus::Running {
            return Err(TaskContractError::IllegalTransition {
                from: self.status.clone(),
            });
        }
        if now < self.updated_at_unix_ms {
            return Err(TaskContractError::TimestampRegression {
                current: self.updated_at_unix_ms,
                attempted: now,
            });
        }
        // Stale and Interrupted have dedicated reducers with specific origin
        // states; record_terminal must not duplicate those paths.
        if matches!(terminal, TaskTerminal::Stale | TaskTerminal::Interrupted) {
            return Err(TaskContractError::UnsupportedTerminal { terminal });
        }
        let status = match terminal {
            TaskTerminal::NeedsUserInput => TaskStatus::NeedsUserInput,
            TaskTerminal::Cancelled => TaskStatus::Cancelled,
            ref other => TaskStatus::Failed {
                terminal: other.clone(),
            },
        };
        let mut next = self.clone();
        next.status = status;
        if let Some(attempt) = next.attempts.last_mut() {
            attempt.finished_at_unix_ms = Some(now);
            attempt.terminal = Some(terminal);
        }
        next.snapshot_revision += 1;
        next.updated_at_unix_ms = now;
        Ok(next)
    }

    /// Transition: ReadyForReview → Applying.
    pub fn begin_apply(&self, now: i64) -> Result<Self, TaskContractError> {
        if self.status != TaskStatus::ReadyForReview {
            return Err(TaskContractError::IllegalTransition {
                from: self.status.clone(),
            });
        }
        if now < self.updated_at_unix_ms {
            return Err(TaskContractError::TimestampRegression {
                current: self.updated_at_unix_ms,
                attempted: now,
            });
        }
        let mut next = self.clone();
        next.status = TaskStatus::Applying;
        next.snapshot_revision += 1;
        next.updated_at_unix_ms = now;
        Ok(next)
    }

    /// Transition: Applying → Completed. Clears pending payload after commit.
    pub fn complete_apply(
        &self,
        receipt: ReviewReceipt,
        now: i64,
    ) -> Result<Self, TaskContractError> {
        if self.status != TaskStatus::Applying {
            return Err(TaskContractError::IllegalTransition {
                from: self.status.clone(),
            });
        }
        if now < self.updated_at_unix_ms {
            return Err(TaskContractError::TimestampRegression {
                current: self.updated_at_unix_ms,
                attempted: now,
            });
        }
        // Validate receipt matches embedded artifact
        if let Some(meta) = &self.artifact_metadata {
            if receipt.artifact_id != meta.artifact_id {
                return Err(TaskContractError::ArtifactMismatch {
                    expected: meta.artifact_id.as_str().to_owned(),
                    got: receipt.artifact_id.as_str().to_owned(),
                });
            }
            if receipt.artifact_revision != meta.artifact_revision {
                return Err(TaskContractError::RevisionMismatch {
                    expected: meta.artifact_revision.get(),
                    got: receipt.artifact_revision.get(),
                });
            }
            if receipt.proposal_id != meta.proposal_id {
                return Err(TaskContractError::ProposalMismatch {
                    expected: meta.proposal_id.clone(),
                    got: receipt.proposal_id.clone(),
                });
            }
        }
        let mut next = self.clone();
        next.status = TaskStatus::Completed;
        next.review_receipt = Some(receipt);
        next.pending_artifact_payload = None;
        next.pending_proposal_payload = None;
        next.snapshot_revision += 1;
        next.updated_at_unix_ms = now;
        Ok(next)
    }

    /// Transition: ReadyForReview → Rejected. Clears pending payload.
    pub fn reject(&self, receipt: ReviewReceipt, now: i64) -> Result<Self, TaskContractError> {
        if self.status != TaskStatus::ReadyForReview {
            return Err(TaskContractError::IllegalTransition {
                from: self.status.clone(),
            });
        }
        if now < self.updated_at_unix_ms {
            return Err(TaskContractError::TimestampRegression {
                current: self.updated_at_unix_ms,
                attempted: now,
            });
        }
        let mut next = self.clone();
        next.status = TaskStatus::Rejected;
        next.review_receipt = Some(receipt);
        next.pending_artifact_payload = None;
        next.pending_proposal_payload = None;
        next.snapshot_revision += 1;
        next.updated_at_unix_ms = now;
        Ok(next)
    }

    /// Transition: ReadyForReview → Stale.
    pub fn mark_stale(&self, now: i64) -> Result<Self, TaskContractError> {
        if self.status != TaskStatus::ReadyForReview {
            return Err(TaskContractError::IllegalTransition {
                from: self.status.clone(),
            });
        }
        if now < self.updated_at_unix_ms {
            return Err(TaskContractError::TimestampRegression {
                current: self.updated_at_unix_ms,
                attempted: now,
            });
        }
        let mut next = self.clone();
        next.status = TaskStatus::Stale;
        next.pending_artifact_payload = None;
        next.pending_proposal_payload = None;
        next.snapshot_revision += 1;
        next.updated_at_unix_ms = now;
        Ok(next)
    }

    /// Reconcile Created, Running, or Applying to Interrupted (startup recovery).
    /// Returns `Ok(None)` if already terminal or not in a reconcilable state.
    pub fn reconcile_interrupted(&self, now: i64) -> Result<Option<Self>, TaskContractError> {
        match self.status {
            TaskStatus::Created | TaskStatus::Running | TaskStatus::Applying => {}
            _ => return Ok(None),
        }
        if now < self.updated_at_unix_ms {
            return Err(TaskContractError::TimestampRegression {
                current: self.updated_at_unix_ms,
                attempted: now,
            });
        }
        let mut next = self.clone();
        next.status = TaskStatus::Interrupted;
        if let Some(attempt) = next.attempts.last_mut() {
            attempt.finished_at_unix_ms = Some(now);
            attempt.terminal = Some(TaskTerminal::Interrupted);
        }
        next.snapshot_revision += 1;
        next.updated_at_unix_ms = now;
        Ok(Some(next))
    }
}

/// Custom Debug that redacts pending payload bytes.
impl fmt::Debug for ProductTaskSnapshot {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ProductTaskSnapshot")
            .field("store_schema_version", &self.store_schema_version)
            .field("snapshot_revision", &self.snapshot_revision)
            .field("task_id", &self.task_id)
            .field("kind", &self.kind)
            .field("source_binding", &self.source_binding)
            .field("status", &self.status)
            .field("attempts", &self.attempts)
            .field("artifact_metadata", &self.artifact_metadata)
            .field(
                "pending_artifact_payload",
                &self.pending_artifact_payload.as_ref().map(|b| b.len()),
            )
            .field(
                "pending_proposal_payload",
                &self.pending_proposal_payload.as_ref().map(|b| b.len()),
            )
            .field("review_receipt", &self.review_receipt)
            .field("created_at_unix_ms", &self.created_at_unix_ms)
            .field("updated_at_unix_ms", &self.updated_at_unix_ms)
            .finish()
    }
}

// ========================================================================
// UUID prefix validation (same algorithm as domain.rs)
// ========================================================================

fn valid_uuid_suffix(value: &str, prefix: &str) -> bool {
    let Some(suffix) = value.strip_prefix(prefix) else {
        return false;
    };
    suffix.len() == 36
        && suffix.bytes().enumerate().all(|(i, b)| match i {
            8 | 13 | 18 | 23 => b == b'-',
            _ => b.is_ascii_hexdigit(),
        })
}

// ========================================================================
// Canonical V1 helpers
// ========================================================================

/// Validate that all floats in the canonical value are finite and
/// all strings/collections are within bounds.
fn validate_canonical_value(value: &serde_json::Value) -> Result<(), CanonicalError> {
    validate_canonical_value_inner(value, true)
}

fn validate_canonical_value_inner(
    value: &serde_json::Value,
    check_null_numbers: bool,
) -> Result<(), CanonicalError> {
    match value {
        serde_json::Value::Number(n) => {
            if let Some(f) = n.as_f64() {
                if !f.is_finite() {
                    return Err(CanonicalError::NonFiniteFloat);
                }
            }
        }
        serde_json::Value::Null if check_null_numbers => {
            // serde_json converts non-finite floats to null;
            // this is caught by pre-validation, but we also
            // reject null in canonical output for safety.
        }
        serde_json::Value::String(s) => {
            if s.len() > MAX_CANONICAL_STRING_LEN {
                return Err(CanonicalError::StringTooLong {
                    len: s.len(),
                    max: MAX_CANONICAL_STRING_LEN,
                });
            }
        }
        serde_json::Value::Array(arr) => {
            if arr.len() > MAX_CANONICAL_COLLECTION_LEN {
                return Err(CanonicalError::CollectionTooLarge {
                    len: arr.len(),
                    max: MAX_CANONICAL_COLLECTION_LEN,
                });
            }
            for item in arr {
                validate_canonical_value_inner(item, check_null_numbers)?;
            }
        }
        serde_json::Value::Object(map) => {
            if map.len() > MAX_CANONICAL_COLLECTION_LEN {
                return Err(CanonicalError::CollectionTooLarge {
                    len: map.len(),
                    max: MAX_CANONICAL_COLLECTION_LEN,
                });
            }
            for (_, v) in map {
                validate_canonical_value_inner(v, check_null_numbers)?;
            }
        }
        _ => {}
    }
    Ok(())
}

/// Convert a serde_json::Value to canonical form: objects become BTreeMaps
/// (sorted keys), arrays keep order, all values recursively canonicalized.
fn canonicalize_value(value: serde_json::Value) -> serde_json::Value {
    match value {
        serde_json::Value::Object(map) => {
            let btree: BTreeMap<String, serde_json::Value> = map.into_iter().collect();
            let canonical: serde_json::Map<String, serde_json::Value> = btree
                .into_iter()
                .map(|(k, v)| (k, canonicalize_value(v)))
                .collect();
            serde_json::Value::Object(canonical)
        }
        serde_json::Value::Array(arr) => {
            serde_json::Value::Array(arr.into_iter().map(canonicalize_value).collect())
        }
        other => other,
    }
}

/// Pre-validate Rust f32/f64 values are finite before JSON conversion,
/// since serde_json silently converts non-finite floats to null.
pub trait ValidateFinite {
    fn check_finite(&self) -> Result<(), CanonicalError>;
}

impl ValidateFinite for f32 {
    fn check_finite(&self) -> Result<(), CanonicalError> {
        if self.is_finite() {
            Ok(())
        } else {
            Err(CanonicalError::NonFiniteFloat)
        }
    }
}

impl ValidateFinite for f64 {
    fn check_finite(&self) -> Result<(), CanonicalError> {
        if self.is_finite() {
            Ok(())
        } else {
            Err(CanonicalError::NonFiniteFloat)
        }
    }
}

impl ValidateFinite for PayloadDryRunV1 {
    fn check_finite(&self) -> Result<(), CanonicalError> {
        self.affected_area.check_finite()
    }
}

impl ValidateFinite for PayloadSourceV1 {
    fn check_finite(&self) -> Result<(), CanonicalError> {
        Ok(())
    }
}

impl ValidateFinite for PayloadProposalV1 {
    fn check_finite(&self) -> Result<(), CanonicalError> {
        Ok(())
    }
}

impl ValidateFinite for PayloadConfigV1 {
    fn check_finite(&self) -> Result<(), CanonicalError> {
        Ok(())
    }
}

impl ValidateFinite for SmartRedactionReviewPayload {
    fn check_finite(&self) -> Result<(), CanonicalError> {
        self.source.check_finite()?;
        self.proposal.check_finite()?;
        self.dry_run.check_finite()?;
        self.config.check_finite()
    }
}

impl ValidateFinite for AnnotationV1 {
    fn check_finite(&self) -> Result<(), CanonicalError> {
        self.x.check_finite()?;
        self.y.check_finite()?;
        self.w.check_finite()?;
        self.h.check_finite()
    }
}

impl ValidateFinite for AnnotationStateV1 {
    fn check_finite(&self) -> Result<(), CanonicalError> {
        for ann in &self.annotations {
            ann.check_finite()?;
        }
        Ok(())
    }
}

impl ValidateFinite for RunConfigFingerprintV1 {
    fn check_finite(&self) -> Result<(), CanonicalError> {
        Ok(())
    }
}

/// Serialize a serde-serializable value to canonical V1 bytes:
/// validates, canonicalizes (sorted object keys), serializes via serde_json.
pub fn canonical_v1_bytes<T: Serialize + ValidateFinite>(
    value: &T,
) -> Result<Vec<u8>, CanonicalError> {
    value.check_finite()?;
    let json_value =
        serde_json::to_value(value).map_err(|e| CanonicalError::Serialization(e.to_string()))?;
    validate_canonical_value(&json_value)?;
    let canonical = canonicalize_value(json_value);
    serde_json::to_vec(&canonical).map_err(|e| CanonicalError::Serialization(e.to_string()))
}

/// SHA-256 hex digest of canonical V1 bytes.
pub fn canonical_v1_digest<T: Serialize + ValidateFinite>(
    value: &T,
) -> Result<String, CanonicalError> {
    let bytes = canonical_v1_bytes(value)?;
    let hash = Sha256::digest(&bytes);
    Ok(hex_encode(&hash))
}

// -- Specific canonical helpers used by tests and production code --

/// Canonical bytes for a config DTO (excludes secrets by construction of
/// RunConfigFingerprintV1 which has no secret fields).
pub fn canonical_config_bytes(config: &RunConfigFingerprintV1) -> Result<Vec<u8>, CanonicalError> {
    canonical_v1_bytes(config)
}

/// Canonical digest for config.
pub fn canonical_config_digest(config: &RunConfigFingerprintV1) -> Result<String, CanonicalError> {
    canonical_v1_digest(config)
}

/// Canonical bytes for a payload DTO.
pub fn canonical_payload_bytes(
    payload: &SmartRedactionReviewPayload,
) -> Result<Vec<u8>, CanonicalError> {
    canonical_v1_bytes(payload)
}

/// Canonical digest for payload.
pub fn canonical_payload_digest(
    payload: &SmartRedactionReviewPayload,
) -> Result<String, CanonicalError> {
    canonical_v1_digest(payload)
}

/// Canonical bytes for proposal DTO.
pub fn canonical_proposal_bytes(proposal: &PayloadProposalV1) -> Result<Vec<u8>, CanonicalError> {
    canonical_v1_bytes(proposal)
}

/// Canonical digest for proposal.
pub fn canonical_proposal_digest(proposal: &PayloadProposalV1) -> Result<String, CanonicalError> {
    canonical_v1_digest(proposal)
}

/// Canonical bytes for source DTO.
pub fn canonical_source_bytes(source: &PayloadSourceV1) -> Result<Vec<u8>, CanonicalError> {
    canonical_v1_bytes(source)
}

/// Canonical digest for source.
pub fn canonical_source_digest(source: &PayloadSourceV1) -> Result<String, CanonicalError> {
    canonical_v1_digest(source)
}

/// Compute annotation-state digest from the DTO.
pub fn compute_annotation_state_digest(
    state: &AnnotationStateV1,
) -> Result<[u8; 32], CanonicalError> {
    let bytes = canonical_v1_bytes(state)?;
    let hash = Sha256::digest(&bytes);
    let mut out = [0u8; 32];
    out.copy_from_slice(&hash);
    Ok(out)
}

/// Canonical bytes for annotation-state DTO.
pub fn canonical_annotation_state_bytes(
    state: &AnnotationStateV1,
) -> Result<Vec<u8>, CanonicalError> {
    canonical_v1_bytes(state)
}

/// Hex digest of annotation-state.
pub fn canonical_annotation_state_digest(
    state: &AnnotationStateV1,
) -> Result<String, CanonicalError> {
    let digest_bytes = compute_annotation_state_digest(state)?;
    Ok(hex_encode(&digest_bytes))
}

// -- V2 canonical helpers --

/// Canonical bytes for a V2 config fingerprint.
pub fn canonical_config_v2_bytes(
    config: &RunConfigFingerprintV2,
) -> Result<Vec<u8>, CanonicalError> {
    canonical_v1_bytes(config)
}

/// Canonical digest for V2 config.
pub fn canonical_config_v2_digest(
    config: &RunConfigFingerprintV2,
) -> Result<String, CanonicalError> {
    canonical_v1_digest(config)
}

/// Hex-encode a byte slice as lowercase.
fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

// ========================================================================
// Tests
// ========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // ------------------------------------------------------------------
    // Test fixtures
    // ------------------------------------------------------------------

    fn task_id_fixture() -> ProductTaskId {
        ProductTaskId::parse("task-00000000-0000-4000-8000-000000000001").unwrap()
    }

    fn run_id_fixture() -> RunId {
        RunId::parse("run-00000000-0000-4000-8000-000000000001").unwrap()
    }

    fn artifact_id_fixture() -> ArtifactId {
        ArtifactId::parse("artifact-00000000-0000-4000-8000-000000000001").unwrap()
    }

    fn source_binding_fixture() -> SourceBinding {
        SourceBinding::smart_redaction([1u8; 32], [2u8; 32], 0, "preset-001".to_owned(), None)
    }

    fn attempt_fixture() -> TaskAttempt {
        TaskAttempt::new(TaskAttemptId::new(1), run_id_fixture(), 10)
    }

    fn _attempt_fixture_with_run(run_id: RunId) -> TaskAttempt {
        TaskAttempt::new(TaskAttemptId::new(1), run_id, 10)
    }

    fn payload_source_fixture() -> PayloadSourceV1 {
        PayloadSourceV1 {
            kind: "smart_redaction".to_owned(),
            validation_summary: "all_valid".to_owned(),
        }
    }

    fn payload_proposal_fixture() -> PayloadProposalV1 {
        PayloadProposalV1 {
            proposal_id: "proposal-00000000-0000-4000-8000-000000000001".to_owned(),
            candidate_count: 3,
        }
    }

    fn payload_fixture() -> SmartRedactionReviewPayload {
        SmartRedactionReviewPayload {
            source: payload_source_fixture(),
            proposal: payload_proposal_fixture(),
            dry_run: PayloadDryRunV1 {
                candidate_count: 3,
                affected_area: 0.42,
            },
            config: PayloadConfigV1 {
                provider: "anthropic".to_owned(),
                model: "claude-sonnet-4-20250514".to_owned(),
                payload_mode: PayloadMode::Author,
                run_kind: "smart_redaction".to_owned(),
                budget_dimensions: {
                    let mut m = BTreeMap::new();
                    m.insert("wall_time_ms".to_owned(), 30_000);
                    m.insert("model_calls".to_owned(), 10);
                    m
                },
            },
        }
    }

    fn metadata_fixture(run_id: RunId, attempt_id: TaskAttemptId) -> ProductArtifactMetadata {
        let payload = payload_fixture();
        let payload_bytes = canonical_payload_bytes(&payload).unwrap();
        let payload_sha = {
            let hash = Sha256::digest(&payload_bytes);
            hex_encode(&hash)
        };
        let config = RunConfigFingerprintV1 {
            provider: "anthropic".to_owned(),
            model: "claude-sonnet-4-20250514".to_owned(),
            payload_mode: PayloadMode::Author,
            run_kind: "smart_redaction".to_owned(),
            budget_dimensions: {
                let mut m = BTreeMap::new();
                m.insert("wall_time_ms".to_owned(), 30_000);
                m.insert("model_calls".to_owned(), 10);
                m
            },
        };
        let config_digest = canonical_config_digest(&config).unwrap();

        ProductArtifactMetadata::new(
            artifact_id_fixture(),
            ArtifactRevision::new(1),
            ArtifactKind::SmartRedaction,
            1,
            payload_sha,
            source_binding_fixture(),
            task_id_fixture(),
            attempt_id,
            run_id,
            "proposal-00000000-0000-4000-8000-000000000001".to_owned(),
            "anthropic".to_owned(),
            "claude-sonnet-4-20250514".to_owned(),
            config_digest,
            3,
            0.42,
            15,
        )
    }

    fn reject_receipt_fixture() -> ReviewReceipt {
        ReviewReceipt {
            artifact_id: artifact_id_fixture(),
            artifact_revision: ArtifactRevision::new(1),
            proposal_id: "proposal-00000000-0000-4000-8000-000000000001".to_owned(),
            applied_candidates: vec![],
            rejected_candidates: vec![0, 1, 2],
            local_delta: LocalReviewDeltaV1 {
                moved_candidates: vec![],
                manual_additions: vec![],
            },
            resulting_document_state_id: None,
            resulting_document_digest: None,
            decided_at_unix_ms: 40,
        }
    }

    fn apply_receipt_fixture() -> ReviewReceipt {
        ReviewReceipt {
            artifact_id: artifact_id_fixture(),
            artifact_revision: ArtifactRevision::new(1),
            proposal_id: "proposal-00000000-0000-4000-8000-000000000001".to_owned(),
            applied_candidates: vec![0, 1, 2],
            rejected_candidates: vec![],
            local_delta: LocalReviewDeltaV1 {
                moved_candidates: vec![],
                manual_additions: vec![],
            },
            resulting_document_state_id: Some(1),
            resulting_document_digest: Some([3u8; 32]),
            decided_at_unix_ms: 50,
        }
    }

    fn created_task_fixture() -> ProductTaskSnapshot {
        ProductTaskSnapshot::new(
            task_id_fixture(),
            TaskKind::SmartRedactionAuthor,
            source_binding_fixture(),
            10,
        )
        .unwrap()
    }

    fn running_task_fixture() -> ProductTaskSnapshot {
        created_task_fixture()
            .start_attempt(attempt_fixture(), 20)
            .unwrap()
    }

    fn ready_task_fixture() -> ProductTaskSnapshot {
        let running = running_task_fixture();
        let meta = metadata_fixture(run_id_fixture(), TaskAttemptId::new(1));
        running
            .record_ready_for_review(meta, payload_fixture(), None, 30)
            .unwrap()
    }

    fn applying_task_fixture() -> ProductTaskSnapshot {
        ready_task_fixture().begin_apply(35).unwrap()
    }

    fn annotations_fixture() -> AnnotationStateV1 {
        AnnotationStateV1 {
            width: 100,
            height: 80,
            state_id: 0,
            annotations: vec![AnnotationV1 {
                kind: "redact".to_owned(),
                x: 10.0,
                y: 20.0,
                w: 30.0,
                h: 40.0,
            }],
        }
    }

    fn config_fixture(entries: [(&str, &str); 2], _secret: Option<&str>) -> RunConfigFingerprintV1 {
        let mut budget = BTreeMap::new();
        for (k, v) in entries {
            budget.insert(k.to_owned(), v.parse::<u64>().unwrap());
        }
        // secret is intentionally NOT part of RunConfigFingerprintV1
        RunConfigFingerprintV1 {
            provider: "anthropic".to_owned(),
            model: "claude-sonnet-4-20250514".to_owned(),
            payload_mode: PayloadMode::Author,
            run_kind: "smart_redaction".to_owned(),
            budget_dimensions: budget,
        }
    }

    fn document_binding(
        base_image: [u8; 32],
        annotations: AnnotationStateV1,
        state_id: u32,
    ) -> Result<DocumentContentBinding, CanonicalError> {
        DocumentContentBinding::new(base_image, &annotations, state_id)
    }

    // ------------------------------------------------------------------
    // Step 1: Reducer and privacy tests
    // ------------------------------------------------------------------

    #[test]
    fn reducers_increment_exact_snapshot_revision() {
        let created = created_task_fixture();
        let running = created.start_attempt(attempt_fixture(), 20).unwrap();
        assert_eq!(created.snapshot_revision(), 0);
        assert_eq!(running.snapshot_revision(), 1);
        assert_eq!(running.status(), TaskStatus::Running);
    }

    #[test]
    fn terminal_task_cannot_restart() {
        let cancelled = running_task_fixture()
            .record_terminal(TaskTerminal::Cancelled, 30)
            .unwrap();
        assert!(matches!(
            cancelled.start_attempt(attempt_fixture(), 40),
            Err(TaskContractError::IllegalTransition { .. })
        ));
    }

    #[test]
    fn terminal_transition_clears_pending_payload() {
        let ready = ready_task_fixture();
        let rejected = ready.reject(reject_receipt_fixture(), 40).unwrap();
        assert!(rejected.pending_artifact_payload().is_none());
        assert!(rejected.artifact_metadata().is_some());
    }

    #[test]
    fn running_to_interrupted_succeeds() {
        let running = running_task_fixture();
        let interrupted = running.reconcile_interrupted(30).unwrap().unwrap();
        assert_eq!(interrupted.status(), TaskStatus::Interrupted);
        assert_eq!(interrupted.snapshot_revision(), 2);
    }

    #[test]
    fn applying_to_interrupted_succeeds() {
        let applying = applying_task_fixture();
        let interrupted = applying.reconcile_interrupted(40).unwrap().unwrap();
        assert_eq!(interrupted.status(), TaskStatus::Interrupted);
    }

    #[test]
    fn already_terminal_reconcile_returns_none() {
        let cancelled = running_task_fixture()
            .record_terminal(TaskTerminal::Cancelled, 30)
            .unwrap();
        assert!(cancelled.reconcile_interrupted(40).unwrap().is_none());
    }

    #[test]
    fn timestamp_regression_rejected_on_start_attempt() {
        let created = created_task_fixture();
        assert!(matches!(
            created.start_attempt(attempt_fixture(), 5),
            Err(TaskContractError::TimestampRegression { .. })
        ));
    }

    #[test]
    fn timestamp_regression_rejected_on_record_terminal() {
        let running = running_task_fixture();
        assert!(matches!(
            running.record_terminal(TaskTerminal::Cancelled, 5),
            Err(TaskContractError::TimestampRegression { .. })
        ));
    }

    #[test]
    fn timestamp_regression_rejected_on_begin_apply() {
        let ready = ready_task_fixture();
        assert!(matches!(
            ready.begin_apply(10),
            Err(TaskContractError::TimestampRegression { .. })
        ));
    }

    #[test]
    fn mismatched_run_id_rejected_on_record_ready_for_review() {
        let running = running_task_fixture();
        let wrong_run = RunId::parse("run-99999999-9999-4999-8999-999999999999").unwrap();
        let meta = metadata_fixture(wrong_run, TaskAttemptId::new(1));
        assert!(matches!(
            running.record_ready_for_review(meta, payload_fixture(), None, 30),
            Err(TaskContractError::RunMismatch { .. })
        ));
    }

    #[test]
    fn mismatched_artifact_id_rejected_on_complete_apply() {
        let applying = applying_task_fixture();
        let wrong_receipt = ReviewReceipt {
            artifact_id: ArtifactId::parse("artifact-99999999-9999-4999-8999-999999999999")
                .unwrap(),
            ..apply_receipt_fixture()
        };
        assert!(matches!(
            applying.complete_apply(wrong_receipt, 50),
            Err(TaskContractError::ArtifactMismatch { .. })
        ));
    }

    #[test]
    fn mismatched_revision_rejected_on_complete_apply() {
        let applying = applying_task_fixture();
        let wrong_receipt = ReviewReceipt {
            artifact_revision: ArtifactRevision::new(99),
            ..apply_receipt_fixture()
        };
        assert!(matches!(
            applying.complete_apply(wrong_receipt, 50),
            Err(TaskContractError::RevisionMismatch { .. })
        ));
    }

    #[test]
    fn mismatched_proposal_id_rejected_on_complete_apply() {
        let applying = applying_task_fixture();
        let wrong_receipt = ReviewReceipt {
            proposal_id: "proposal-wrong".to_owned(),
            ..apply_receipt_fixture()
        };
        assert!(matches!(
            applying.complete_apply(wrong_receipt, 50),
            Err(TaskContractError::ProposalMismatch { .. })
        ));
    }

    #[test]
    fn debug_redacts_pending_payload() {
        let ready = ready_task_fixture();
        let debug_str = format!("{ready:?}");
        // Debug shows length, not content
        assert!(debug_str.contains("pending_artifact_payload"));
        // Check that raw JSON payload content doesn't appear in Debug
        // Note: "smart_redaction" appears in metadata kind field too, so check JSON format
        assert!(!debug_str.contains("\"kind\""));
        // The pending payload is stored as raw bytes; Debug only shows length
        assert!(!debug_str.contains("validation_summary"));
    }

    #[test]
    fn complete_apply_clears_payload_and_retains_metadata() {
        let applying = applying_task_fixture();
        let completed = applying
            .complete_apply(apply_receipt_fixture(), 50)
            .unwrap();
        assert_eq!(completed.status(), TaskStatus::Completed);
        assert!(completed.pending_artifact_payload().is_none());
        assert!(completed.artifact_metadata().is_some());
        assert!(completed.review_receipt().is_some());
    }

    #[test]
    fn mark_stale_clears_pending_payload() {
        let running = running_task_fixture();
        let meta = metadata_fixture(run_id_fixture(), TaskAttemptId::new(1));
        let ready = running
            .record_ready_for_review(
                meta,
                payload_fixture(),
                Some(b"proposal-bytes".to_vec()),
                30,
            )
            .unwrap();
        let stale = ready.mark_stale(40).unwrap();
        assert_eq!(stale.status(), TaskStatus::Stale);
        assert!(stale.pending_artifact_payload().is_none());
        assert!(stale.pending_proposal_payload().is_none());
        assert!(stale.artifact_metadata().is_some());
    }

    // ------------------------------------------------------------------
    // Step 2: Canonicalization and content-binding tests
    // ------------------------------------------------------------------

    #[test]
    fn canonical_config_digest_ignores_map_insertion_order_and_excludes_secret() {
        let a = config_fixture([("b", "2"), ("a", "1")], Some("secret-a"));
        let b = config_fixture([("a", "1"), ("b", "2")], Some("secret-b"));
        assert_eq!(
            canonical_config_digest(&a).unwrap(),
            canonical_config_digest(&b).unwrap()
        );
        // RunConfigFingerprintV1 has no secret field at all
        assert!(!String::from_utf8(canonical_config_bytes(&a).unwrap())
            .unwrap()
            .contains("secret"));
    }

    #[test]
    fn document_binding_distinguishes_same_state_id_on_different_images() {
        let a = document_binding([1; 32], annotations_fixture(), 0).unwrap();
        let b = document_binding([2; 32], annotations_fixture(), 0).unwrap();
        assert_ne!(a, b);
    }

    #[test]
    fn canonical_payload_rejects_non_finite_values() {
        let mut payload = payload_fixture();
        payload.dry_run.affected_area = f32::NAN;
        assert_eq!(
            canonical_payload_bytes(&payload),
            Err(CanonicalError::NonFiniteFloat)
        );
    }

    #[test]
    fn canonical_payload_rejects_positive_infinity() {
        let mut payload = payload_fixture();
        payload.dry_run.affected_area = f32::INFINITY;
        assert_eq!(
            canonical_payload_bytes(&payload),
            Err(CanonicalError::NonFiniteFloat)
        );
    }

    #[test]
    fn canonical_payload_rejects_negative_infinity() {
        let mut payload = payload_fixture();
        payload.dry_run.affected_area = f32::NEG_INFINITY;
        assert_eq!(
            canonical_payload_bytes(&payload),
            Err(CanonicalError::NonFiniteFloat)
        );
    }

    #[test]
    fn canonical_payload_golden_bytes_and_digest() {
        let payload = payload_fixture();
        let bytes = canonical_payload_bytes(&payload).unwrap();
        let digest = canonical_payload_digest(&payload).unwrap();
        // The canonical bytes should be stable JSON with sorted keys
        let json_str = String::from_utf8(bytes.clone()).unwrap();
        assert!(json_str.contains("\"affected_area\":"));
        assert!(json_str.contains("\"candidate_count\":3"));
        // Digest should be 64 hex chars (SHA-256)
        assert_eq!(digest.len(), 64);
        assert!(digest.chars().all(|c| c.is_ascii_hexdigit()));
        // Golden: same input always produces the same digest
        let digest2 = canonical_payload_digest(&payload).unwrap();
        assert_eq!(digest, digest2);
    }

    #[test]
    fn canonical_proposal_golden_bytes_and_digest() {
        let proposal = payload_proposal_fixture();
        let bytes = canonical_proposal_bytes(&proposal).unwrap();
        let digest = canonical_proposal_digest(&proposal).unwrap();
        assert_eq!(digest.len(), 64);
        // Stable
        assert_eq!(digest, canonical_proposal_digest(&proposal).unwrap());
        let json_str = String::from_utf8(bytes).unwrap();
        assert!(json_str.contains("\"candidate_count\":3"));
    }

    #[test]
    fn canonical_source_golden_bytes_and_digest() {
        let source = payload_source_fixture();
        let bytes = canonical_source_bytes(&source).unwrap();
        let digest = canonical_source_digest(&source).unwrap();
        assert_eq!(digest.len(), 64);
        assert_eq!(digest, canonical_source_digest(&source).unwrap());
        let json_str = String::from_utf8(bytes).unwrap();
        assert!(json_str.contains("smart_redaction"));
    }

    #[test]
    fn canonical_config_golden_bytes_and_digest() {
        let config = RunConfigFingerprintV1 {
            provider: "anthropic".to_owned(),
            model: "claude-sonnet-4-20250514".to_owned(),
            payload_mode: PayloadMode::Author,
            run_kind: "smart_redaction".to_owned(),
            budget_dimensions: {
                let mut m = BTreeMap::new();
                m.insert("model_calls".to_owned(), 10);
                m.insert("wall_time_ms".to_owned(), 30_000);
                m
            },
        };
        let _bytes = canonical_config_bytes(&config).unwrap();
        let digest = canonical_config_digest(&config).unwrap();
        assert_eq!(digest.len(), 64);
        // Same config with different insertion order → same digest
        let config2 = RunConfigFingerprintV1 {
            budget_dimensions: {
                let mut m = BTreeMap::new();
                m.insert("wall_time_ms".to_owned(), 30_000);
                m.insert("model_calls".to_owned(), 10);
                m
            },
            ..config.clone()
        };
        assert_eq!(digest, canonical_config_digest(&config2).unwrap());
    }

    #[test]
    fn canonical_annotation_state_golden_digest() {
        let state = annotations_fixture();
        let bytes = canonical_annotation_state_bytes(&state).unwrap();
        let digest = canonical_annotation_state_digest(&state).unwrap();
        assert_eq!(digest.len(), 64);
        // Stable
        assert_eq!(digest, canonical_annotation_state_digest(&state).unwrap());
        let json_str = String::from_utf8(bytes).unwrap();
        assert!(json_str.contains("\"height\":80"));
    }

    #[test]
    fn annotation_state_distinguishes_different_dimensions() {
        let a = AnnotationStateV1 {
            width: 100,
            height: 80,
            state_id: 0,
            annotations: vec![],
        };
        let b = AnnotationStateV1 {
            width: 200,
            height: 80,
            state_id: 0,
            annotations: vec![],
        };
        assert_ne!(
            canonical_annotation_state_digest(&a).unwrap(),
            canonical_annotation_state_digest(&b).unwrap()
        );
    }

    #[test]
    fn annotation_state_distinguishes_different_state_ids() {
        let a = AnnotationStateV1 {
            width: 100,
            height: 80,
            state_id: 0,
            annotations: vec![],
        };
        let b = AnnotationStateV1 {
            width: 100,
            height: 80,
            state_id: 1,
            annotations: vec![],
        };
        assert_ne!(
            canonical_annotation_state_digest(&a).unwrap(),
            canonical_annotation_state_digest(&b).unwrap()
        );
    }

    // ------------------------------------------------------------------
    // Adversarial tests
    // ------------------------------------------------------------------

    #[test]
    fn canonical_rejects_oversized_string() {
        let long_str = "x".repeat(5000);
        let source = PayloadSourceV1 {
            kind: long_str,
            validation_summary: "ok".to_owned(),
        };
        assert!(matches!(
            canonical_source_bytes(&source),
            Err(CanonicalError::StringTooLong { .. })
        ));
    }

    #[test]
    fn canonical_rejects_oversized_collection() {
        let huge_map: BTreeMap<String, u64> = (0..300u64).map(|i| (format!("k{i}"), i)).collect();
        let config = RunConfigFingerprintV1 {
            provider: "anthropic".to_owned(),
            model: "claude-sonnet-4-20250514".to_owned(),
            payload_mode: PayloadMode::Author,
            run_kind: "smart_redaction".to_owned(),
            budget_dimensions: huge_map,
        };
        assert!(matches!(
            canonical_config_bytes(&config),
            Err(CanonicalError::CollectionTooLarge { .. })
        ));
    }

    #[test]
    fn invalid_task_id_rejected() {
        assert!(ProductTaskId::parse("not-a-uuid").is_err());
        assert!(ProductTaskId::parse("run-00000000-0000-4000-8000-000000000001").is_err());
        assert!(ProductTaskId::parse("task-../escape").is_err());
    }

    #[test]
    fn invalid_artifact_id_rejected() {
        assert!(ArtifactId::parse("not-a-uuid").is_err());
        assert!(ArtifactId::parse("artifact-../escape").is_err());
    }

    #[test]
    fn created_task_has_schema_version_one() {
        let task = created_task_fixture();
        assert_eq!(task.store_schema_version(), 1);
    }

    #[test]
    fn cannot_begin_apply_from_created() {
        let created = created_task_fixture();
        assert!(matches!(
            created.begin_apply(20),
            Err(TaskContractError::IllegalTransition { .. })
        ));
    }

    #[test]
    fn cannot_record_ready_for_review_from_created() {
        let created = created_task_fixture();
        let meta = metadata_fixture(run_id_fixture(), TaskAttemptId::new(1));
        assert!(matches!(
            created.record_ready_for_review(meta, payload_fixture(), None, 20),
            Err(TaskContractError::IllegalTransition { .. })
        ));
    }

    #[test]
    fn cannot_reject_from_running() {
        let running = running_task_fixture();
        assert!(matches!(
            running.reject(reject_receipt_fixture(), 30),
            Err(TaskContractError::IllegalTransition { .. })
        ));
    }

    #[test]
    fn cannot_complete_apply_from_ready_for_review() {
        let ready = ready_task_fixture();
        assert!(matches!(
            ready.complete_apply(apply_receipt_fixture(), 40),
            Err(TaskContractError::IllegalTransition { .. })
        ));
    }

    #[test]
    fn document_binding_same_inputs_produce_same_binding() {
        let a = document_binding([1; 32], annotations_fixture(), 0).unwrap();
        let b = document_binding([1; 32], annotations_fixture(), 0).unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn document_binding_different_state_ids_differ() {
        let a = document_binding([1; 32], annotations_fixture(), 0).unwrap();
        let b = document_binding([1; 32], annotations_fixture(), 1).unwrap();
        assert_ne!(a, b);
    }

    #[test]
    fn snapshot_serde_round_trip() {
        let task = ready_task_fixture();
        let json = serde_json::to_string(&task).unwrap();
        let restored: ProductTaskSnapshot = serde_json::from_str(&json).unwrap();
        assert_eq!(task, restored);
    }

    #[test]
    fn payload_serde_round_trip() {
        let payload = payload_fixture();
        let json = serde_json::to_string(&payload).unwrap();
        let restored: SmartRedactionReviewPayload = serde_json::from_str(&json).unwrap();
        assert_eq!(payload, restored);
    }

    // ------------------------------------------------------------------
    // Finding 1: ConflictingAttempt test
    // ------------------------------------------------------------------

    #[test]
    fn mismatched_attempt_id_rejected_on_record_ready_for_review() {
        let running = running_task_fixture();
        // running_task_fixture uses attempt_id=1; pass metadata with attempt_id=99
        let meta = metadata_fixture(run_id_fixture(), TaskAttemptId::new(99));
        assert!(matches!(
            running.record_ready_for_review(meta, payload_fixture(), None, 30),
            Err(TaskContractError::ConflictingAttempt {
                expected: _,
                got: _
            })
        ));
    }

    // ------------------------------------------------------------------
    // Finding 3: Stale/Interrupted rejected by record_terminal
    // ------------------------------------------------------------------

    #[test]
    fn record_terminal_rejects_stale() {
        let running = running_task_fixture();
        assert!(matches!(
            running.record_terminal(TaskTerminal::Stale, 30),
            Err(TaskContractError::UnsupportedTerminal { .. })
        ));
    }

    #[test]
    fn record_terminal_rejects_interrupted() {
        let running = running_task_fixture();
        assert!(matches!(
            running.record_terminal(TaskTerminal::Interrupted, 30),
            Err(TaskContractError::UnsupportedTerminal { .. })
        ));
    }

    #[test]
    fn record_terminal_allows_other_terminals() {
        let running = running_task_fixture();
        // Cancelled, NeedsUserInput, and Failed variants should still work
        let cancelled = running
            .record_terminal(TaskTerminal::Cancelled, 30)
            .unwrap();
        assert_eq!(cancelled.status(), TaskStatus::Cancelled);

        let needs_input = running_task_fixture()
            .record_terminal(TaskTerminal::NeedsUserInput, 30)
            .unwrap();
        assert_eq!(needs_input.status(), TaskStatus::NeedsUserInput);

        let budget = running_task_fixture()
            .record_terminal(
                TaskTerminal::BudgetExhausted {
                    dimension: "model_calls".to_owned(),
                },
                30,
            )
            .unwrap();
        assert!(matches!(budget.status(), TaskStatus::Failed { .. }));
    }

    // ------------------------------------------------------------------
    // V2: Run contract binding and provenance tests
    // ------------------------------------------------------------------

    fn authority_receipt_fixture() -> crate::authority::AuthoritySnapshotReceiptV1 {
        use crate::authority::{DisclosureCeiling, PreparedCapability, RunOperation};
        crate::authority::AuthoritySnapshotReceiptV1 {
            schema_version: 1,
            task_id: "task-00000000-0000-4000-8000-000000000001".to_owned(),
            attempt_id: 1,
            run_id: "run-00000000-0000-4000-8000-000000000001".to_owned(),
            policy_revision: "rev-1".to_owned(),
            disclosure_ceiling: DisclosureCeiling::FullScreenshot,
            existing_product_capture: false,
            document_binding_digest: "ab".repeat(32),
            prepared_capabilities: vec![PreparedCapability::Ocr],
            granted_operations: vec![RunOperation::SubmitReviewCandidate],
            snapshot_digest: "cd".repeat(32),
            created_at_unix_ms: 10,
        }
    }

    fn skill_use_receipt_fixture() -> crate::skills::SkillUseReceiptV1 {
        crate::skills::SkillUseReceiptV1 {
            schema_version: 1,
            source_authority: "authority://test".to_owned(),
            package_id: "package-1".to_owned(),
            main_resource_id: "resource-1".to_owned(),
            package_digest: "ab".repeat(32),
            declared_version: Some("1.0.0".to_owned()),
            invocation_kind: crate::skills::SkillInvocationKind::HostExplicit,
            resolved_at_unix_ms: 10,
        }
    }

    fn run_contract_fixture(_running: &ProductTaskSnapshot) -> RunContractReceiptV1 {
        RunContractReceiptV1 {
            authority: authority_receipt_fixture(),
            skill_use: skill_use_receipt_fixture(),
            bound_at_unix_ms: 20,
        }
    }

    fn run_contract_with_skill_digest(
        _running: &ProductTaskSnapshot,
        digest: &str,
    ) -> RunContractReceiptV1 {
        RunContractReceiptV1 {
            authority: authority_receipt_fixture(),
            skill_use: crate::skills::SkillUseReceiptV1 {
                package_digest: digest.to_owned(),
                ..skill_use_receipt_fixture()
            },
            bound_at_unix_ms: 20,
        }
    }

    fn running_with_contract_fixture() -> ProductTaskSnapshot {
        let running = running_task_fixture();
        let receipt = run_contract_fixture(&running);
        running.bind_run_contract(receipt, 25).unwrap()
    }

    fn v2_metadata_with_contract(contract: &RunContractReceiptV1) -> ProductArtifactMetadata {
        let payload = payload_fixture();
        let payload_bytes = canonical_payload_bytes(&payload).unwrap();
        let payload_sha = {
            let hash = Sha256::digest(&payload_bytes);
            hex_encode(&hash)
        };
        let config = RunConfigFingerprintV2 {
            provider: "anthropic".to_owned(),
            model: "claude-sonnet-4-20250514".to_owned(),
            payload_mode: PayloadMode::Author,
            run_kind: "smart_redaction".to_owned(),
            budget_dimensions: {
                let mut m = BTreeMap::new();
                m.insert("wall_time_ms".to_owned(), 30_000);
                m.insert("model_calls".to_owned(), 10);
                m
            },
            authority_snapshot_digest: contract.authority.snapshot_digest.clone(),
            skill_use: contract.skill_use.clone(),
        };
        let config_digest = canonical_config_v2_digest(&config).unwrap();
        ProductArtifactMetadata::new_v2(
            artifact_id_fixture(),
            ArtifactRevision::new(1),
            ArtifactKind::SmartRedaction,
            2,
            payload_sha,
            source_binding_fixture(),
            task_id_fixture(),
            TaskAttemptId::new(1),
            run_id_fixture(),
            "proposal-00000000-0000-4000-8000-000000000001".to_owned(),
            "anthropic".to_owned(),
            "claude-sonnet-4-20250514".to_owned(),
            config_digest,
            3,
            0.42,
            15,
            contract.clone(),
        )
    }

    #[test]
    fn run_contract_binds_once_to_active_attempt_before_promotion() {
        let running = running_task_fixture();
        let receipt = run_contract_fixture(&running);
        let bound = running.bind_run_contract(receipt.clone(), 20).unwrap();
        assert_eq!(
            bound.attempts().last().unwrap().run_contract(),
            Some(&receipt)
        );
        assert_eq!(bound.snapshot_revision(), running.snapshot_revision() + 1);

        // Second conflicting receipt → RunContractConflict
        let conflict = run_contract_with_skill_digest(&running, &"ff".repeat(32));
        assert!(matches!(
            bound.bind_run_contract(conflict, 21),
            Err(TaskContractError::RunContractConflict)
        ));
    }

    #[test]
    fn bind_run_contract_idempotent_on_identical_receipt() {
        let running = running_task_fixture();
        let receipt = run_contract_fixture(&running);
        let bound = running.bind_run_contract(receipt.clone(), 20).unwrap();
        let revision_after_first = bound.snapshot_revision();
        // Identical receipt → no revision bump
        let again = bound.bind_run_contract(receipt, 21).unwrap();
        assert_eq!(again.snapshot_revision(), revision_after_first);
        assert_eq!(again, bound);
    }

    #[test]
    fn promotion_requires_and_copies_exact_run_contract() {
        let bound = running_with_contract_fixture();
        let contract = bound.active_run_contract().unwrap().clone();
        let metadata = v2_metadata_with_contract(&contract);
        let ready = bound
            .record_ready_for_review(metadata, payload_fixture(), None, 30)
            .unwrap();
        assert_eq!(
            ready.artifact_metadata().unwrap().run_contract(),
            bound.active_run_contract()
        );
    }

    #[test]
    fn v2_promotion_rejected_without_run_contract() {
        let running_v2 = ProductTaskSnapshot::new_v2(
            task_id_fixture(),
            TaskKind::SmartRedactionAuthor,
            source_binding_fixture(),
            10,
        )
        .unwrap()
        .start_attempt(attempt_fixture(), 20)
        .unwrap();
        // No bind_run_contract → MissingRunContract
        let meta = metadata_fixture(run_id_fixture(), TaskAttemptId::new(1));
        assert!(matches!(
            running_v2.record_ready_for_review(meta, payload_fixture(), None, 30),
            Err(TaskContractError::MissingRunContract)
        ));
    }

    #[test]
    fn v1_promotion_still_works_without_run_contract() {
        let running = running_task_fixture(); // schema 1
        let meta = metadata_fixture(run_id_fixture(), TaskAttemptId::new(1));
        let ready = running
            .record_ready_for_review(meta, payload_fixture(), None, 30)
            .unwrap();
        assert_eq!(ready.status(), TaskStatus::ReadyForReview);
    }

    #[test]
    fn bind_run_contract_rejected_when_not_running() {
        let created = created_task_fixture();
        let receipt = run_contract_fixture(&created);
        assert!(matches!(
            created.bind_run_contract(receipt, 20),
            Err(TaskContractError::IllegalTransition { .. })
        ));
    }

    #[test]
    fn bind_run_contract_rejected_on_terminal_attempt() {
        let cancelled = running_task_fixture()
            .record_terminal(TaskTerminal::Cancelled, 30)
            .unwrap();
        let receipt = run_contract_fixture(&cancelled);
        assert!(matches!(
            cancelled.bind_run_contract(receipt, 40),
            Err(TaskContractError::IllegalTransition { .. })
        ));
    }

    #[test]
    fn bind_run_contract_rejected_on_timestamp_regression() {
        let running = running_task_fixture();
        let receipt = run_contract_fixture(&running);
        assert!(matches!(
            running.bind_run_contract(receipt, 5),
            Err(TaskContractError::TimestampRegression { .. })
        ));
    }

    #[test]
    fn bind_run_contract_rejected_on_run_mismatch() {
        let running = running_task_fixture();
        let mut receipt = run_contract_fixture(&running);
        receipt.authority.run_id = "run-99999999-9999-4999-8999-999999999999".to_owned();
        assert!(matches!(
            running.bind_run_contract(receipt, 20),
            Err(TaskContractError::RunMismatch { .. })
        ));
    }

    #[test]
    fn bind_run_contract_rejected_on_task_mismatch() {
        let running = running_task_fixture();
        let mut receipt = run_contract_fixture(&running);
        receipt.authority.task_id = "task-99999999-9999-4999-8999-999999999999".to_owned();
        assert!(matches!(
            running.bind_run_contract(receipt, 20),
            Err(TaskContractError::RunMismatch { .. })
        ));
    }

    #[test]
    fn bind_run_contract_rejected_on_attempt_mismatch() {
        let running = running_task_fixture();
        let mut receipt = run_contract_fixture(&running);
        receipt.authority.attempt_id = 99;
        assert!(matches!(
            running.bind_run_contract(receipt, 20),
            Err(TaskContractError::ConflictingAttempt { .. })
        ));
    }

    #[test]
    fn v2_task_created_with_schema_version_two() {
        let task = ProductTaskSnapshot::new_v2(
            task_id_fixture(),
            TaskKind::SmartRedactionAuthor,
            source_binding_fixture(),
            10,
        )
        .unwrap();
        assert_eq!(task.store_schema_version(), 2);
        assert_eq!(task.snapshot_revision(), 0);
    }

    #[test]
    fn v2_metadata_carries_run_contract() {
        let contract = RunContractReceiptV1 {
            authority: authority_receipt_fixture(),
            skill_use: skill_use_receipt_fixture(),
            bound_at_unix_ms: 20,
        };
        let meta = v2_metadata_with_contract(&contract);
        assert_eq!(meta.run_contract(), Some(&contract));
    }

    // ------------------------------------------------------------------
    // V1-compatible deserialization (old JSON without new fields)
    // ------------------------------------------------------------------

    #[test]
    fn v1_json_deserializes_with_run_contract_none() {
        let running = running_task_fixture();
        let meta = metadata_fixture(run_id_fixture(), TaskAttemptId::new(1));
        let ready = running
            .record_ready_for_review(meta, payload_fixture(), None, 30)
            .unwrap();
        let json = serde_json::to_string(&ready).unwrap();

        // V1 JSON should NOT contain "run_contract" at all.
        assert!(!json.contains("run_contract"));

        // Deserialize: run_contract should be None.
        let restored: ProductTaskSnapshot = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.store_schema_version(), 1);
        assert!(restored.active_run_contract().is_none());
        assert_eq!(restored, ready);
    }

    #[test]
    fn v1_attempt_json_deserializes_without_run_contract() {
        let attempt = attempt_fixture();
        let json = serde_json::to_string(&attempt).unwrap();
        assert!(!json.contains("run_contract"));

        let restored: TaskAttempt = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.run_contract(), None);
        assert_eq!(restored, attempt);
    }

    #[test]
    fn v1_metadata_json_deserializes_without_run_contract() {
        let meta = metadata_fixture(run_id_fixture(), TaskAttemptId::new(1));
        let json = serde_json::to_string(&meta).unwrap();
        assert!(!json.contains("run_contract"));

        let restored: ProductArtifactMetadata = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.run_contract(), None);
        assert_eq!(restored, meta);
    }

    // ------------------------------------------------------------------
    // V2 canonical golden vectors and privacy scans
    // ------------------------------------------------------------------

    #[test]
    fn canonical_config_v2_golden_bytes_and_digest() {
        let config = RunConfigFingerprintV2 {
            provider: "anthropic".to_owned(),
            model: "claude-sonnet-4-20250514".to_owned(),
            payload_mode: PayloadMode::Author,
            run_kind: "smart_redaction".to_owned(),
            budget_dimensions: {
                let mut m = BTreeMap::new();
                m.insert("model_calls".to_owned(), 10);
                m.insert("wall_time_ms".to_owned(), 30_000);
                m
            },
            authority_snapshot_digest: "ab".repeat(32),
            skill_use: skill_use_receipt_fixture(),
        };
        let bytes = canonical_config_v2_bytes(&config).unwrap();
        let digest = canonical_config_v2_digest(&config).unwrap();
        assert_eq!(digest.len(), 64);
        assert!(digest.chars().all(|c| c.is_ascii_hexdigit()));
        assert_eq!(digest, canonical_config_v2_digest(&config).unwrap());
        let json_str = String::from_utf8(bytes).unwrap();
        assert!(json_str.contains("authority_snapshot_digest"));
        assert!(json_str.contains("skill_use"));
    }

    #[test]
    fn canonical_config_v2_ignores_budget_insertion_order() {
        let base = RunConfigFingerprintV2 {
            provider: "anthropic".to_owned(),
            model: "claude-sonnet-4-20250514".to_owned(),
            payload_mode: PayloadMode::Author,
            run_kind: "smart_redaction".to_owned(),
            budget_dimensions: {
                let mut m = BTreeMap::new();
                m.insert("a".to_owned(), 1);
                m.insert("b".to_owned(), 2);
                m
            },
            authority_snapshot_digest: "ab".repeat(32),
            skill_use: skill_use_receipt_fixture(),
        };
        let reordered = RunConfigFingerprintV2 {
            budget_dimensions: {
                let mut m = BTreeMap::new();
                m.insert("b".to_owned(), 2);
                m.insert("a".to_owned(), 1);
                m
            },
            ..base.clone()
        };
        assert_eq!(
            canonical_config_v2_digest(&base).unwrap(),
            canonical_config_v2_digest(&reordered).unwrap()
        );
    }

    #[test]
    fn canonical_config_v2_differs_from_v1() {
        let v1 = RunConfigFingerprintV1 {
            provider: "anthropic".to_owned(),
            model: "claude-sonnet-4-20250514".to_owned(),
            payload_mode: PayloadMode::Author,
            run_kind: "smart_redaction".to_owned(),
            budget_dimensions: {
                let mut m = BTreeMap::new();
                m.insert("wall_time_ms".to_owned(), 30_000);
                m
            },
        };
        let v2 = RunConfigFingerprintV2 {
            provider: "anthropic".to_owned(),
            model: "claude-sonnet-4-20250514".to_owned(),
            payload_mode: PayloadMode::Author,
            run_kind: "smart_redaction".to_owned(),
            budget_dimensions: {
                let mut m = BTreeMap::new();
                m.insert("wall_time_ms".to_owned(), 30_000);
                m
            },
            authority_snapshot_digest: "ab".repeat(32),
            skill_use: skill_use_receipt_fixture(),
        };
        let v1_digest = canonical_config_digest(&v1).unwrap();
        let v2_digest = canonical_config_v2_digest(&v2).unwrap();
        assert_ne!(v1_digest, v2_digest);
    }

    #[test]
    fn run_contract_receipt_serde_round_trip() {
        let receipt = RunContractReceiptV1 {
            authority: authority_receipt_fixture(),
            skill_use: skill_use_receipt_fixture(),
            bound_at_unix_ms: 20,
        };
        let json = serde_json::to_string(&receipt).unwrap();
        let restored: RunContractReceiptV1 = serde_json::from_str(&json).unwrap();
        assert_eq!(receipt, restored);
    }

    #[test]
    fn v2_full_lifecycle_round_trip() {
        let task = ProductTaskSnapshot::new_v2(
            task_id_fixture(),
            TaskKind::SmartRedactionAuthor,
            source_binding_fixture(),
            10,
        )
        .unwrap();
        let running = task.start_attempt(attempt_fixture(), 20).unwrap();
        let receipt = run_contract_fixture(&running);
        let bound = running.bind_run_contract(receipt.clone(), 25).unwrap();
        let meta = v2_metadata_with_contract(&receipt);
        let ready = bound
            .record_ready_for_review(meta, payload_fixture(), None, 30)
            .unwrap();
        let applying = ready.begin_apply(35).unwrap();
        let completed = applying
            .complete_apply(
                ReviewReceipt {
                    artifact_id: artifact_id_fixture(),
                    artifact_revision: ArtifactRevision::new(1),
                    proposal_id: "proposal-00000000-0000-4000-8000-000000000001".to_owned(),
                    applied_candidates: vec![0, 1, 2],
                    rejected_candidates: vec![],
                    local_delta: LocalReviewDeltaV1 {
                        moved_candidates: vec![],
                        manual_additions: vec![],
                    },
                    resulting_document_state_id: Some(1),
                    resulting_document_digest: Some([3u8; 32]),
                    decided_at_unix_ms: 50,
                },
                50,
            )
            .unwrap();
        assert_eq!(completed.status(), TaskStatus::Completed);
        assert_eq!(completed.store_schema_version(), 2);
        assert!(completed.active_run_contract().is_some());
        assert_eq!(completed.active_run_contract(), Some(&receipt));

        // JSON round-trip preserves run contract
        let json = serde_json::to_string(&completed).unwrap();
        let restored: ProductTaskSnapshot = serde_json::from_str(&json).unwrap();
        assert_eq!(restored, completed);
    }

    #[test]
    fn v2_fingerprint_privacy_no_secrets() {
        let config = RunConfigFingerprintV2 {
            provider: "anthropic".to_owned(),
            model: "claude-sonnet-4-20250514".to_owned(),
            payload_mode: PayloadMode::Author,
            run_kind: "smart_redaction".to_owned(),
            budget_dimensions: BTreeMap::new(),
            authority_snapshot_digest: "ab".repeat(32),
            skill_use: skill_use_receipt_fixture(),
        };
        let bytes = canonical_config_v2_bytes(&config).unwrap();
        let json_str = String::from_utf8(bytes).unwrap();
        assert!(!json_str.contains("secret"));
        assert!(!json_str.contains("api_key"));
        assert!(!json_str.contains("password"));
        assert!(!json_str.contains("/home/"));
    }

    #[test]
    fn source_binding_round_trips_all_variants() {
        let cases = vec![
            SourceBinding::smart_redaction([1u8; 32], [2u8; 32], 7, "p".into(), None),
            SourceBinding::ActionGuideProject {
                project_root_sha256: [3u8; 32],
                revision: 9,
                projection_digest: "ab".repeat(32),
            },
            SourceBinding::ActionGuideEphemeralGuide {
                guide_digest: "cd".repeat(32),
            },
        ];

        for case in cases {
            let json = serde_json::to_string(&case).unwrap();
            let back: SourceBinding = serde_json::from_str(&json).unwrap();
            assert_eq!(case, back, "round trip failed for {json}");
        }
    }
}
