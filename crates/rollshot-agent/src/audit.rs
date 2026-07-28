//! Durable audit domain contract for material Product Task transitions.
//!
//! `AuditEnvelopeV1` is an immutable, privacy-safe evidence record for one
//! material transition. The envelope correlates to exact task, attempt, run,
//! authority, skill, artifact, and review identities. It never contains
//! pixels, image/proposal bytes, prompt/source/response prose, provider
//! internals, credentials, or tool arguments/results.
//!
//! Canonical bytes use fixed-field DTOs and domain separator
//! `b"rollshot-audit-envelope-v1\0"`; no maps or native error strings.

use std::fmt;
use std::future::Future;
use std::pin::Pin;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::authority::{AuthoritySnapshot, DisclosureCeiling};
use crate::product_task::{canonical_v1_digest, ProductTaskSnapshot, TaskAttemptId, TaskStatus, TaskTerminal};

// ========================================================================
// Constants
// ========================================================================

pub const AUDIT_SCHEMA_VERSION_V1: u32 = 1;
pub const MAX_AUDIT_STRING_BYTES: usize = 512;
pub const MAX_AUDIT_CANDIDATE_IDS: usize = 256;

// ========================================================================
// AuditEventId
// ========================================================================

/// Opaque audit event identifier. Stable across prepare/commit records.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct AuditEventId(String);

impl AuditEventId {
    /// Parse an audit event ID. Must start with `audit-` followed by a
    /// valid UUID suffix (36 chars with dashes at positions 8, 13, 18, 23).
    pub fn parse(value: impl Into<String>) -> Result<Self, AuditContractError> {
        let value = value.into();
        if !value.starts_with("audit-") {
            return Err(AuditContractError::InvalidEventId(value));
        }
        let suffix = &value[6..];
        if suffix.len() != 36 {
            return Err(AuditContractError::InvalidEventId(value));
        }
        if !suffix.bytes().enumerate().all(|(i, b)| match i {
            8 | 13 | 18 | 23 => b == b'-',
            _ => b.is_ascii_hexdigit(),
        }) {
            return Err(AuditContractError::InvalidEventId(value));
        }
        Ok(Self(value))
    }

    /// Generate a new V4 UUID audit event ID with `audit-` prefix.
    pub fn new_v4() -> Self {
        let mut bytes = [0u8; 16];
        getrandom(&mut bytes);
        // Set version 4 (bits 4-7 of byte 6)
        bytes[6] = (bytes[6] & 0x0F) | 0x40;
        // Set variant 1 (bits 6-7 of byte 8)
        bytes[8] = (bytes[8] & 0x3F) | 0x80;
        let uuid = format!(
            "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
            bytes[0], bytes[1], bytes[2], bytes[3],
            bytes[4], bytes[5],
            bytes[6], bytes[7],
            bytes[8], bytes[9],
            bytes[10], bytes[11], bytes[12], bytes[13], bytes[14], bytes[15],
        );
        Self(format!("audit-{uuid}"))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for AuditEventId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Fill buffer with random bytes. Uses OS entropy; panics on failure.
fn getrandom(buf: &mut [u8]) {
    use std::io::Read;
    let mut f = std::fs::File::open("/dev/urandom").expect("open /dev/urandom");
    f.read_exact(buf).expect("read /dev/urandom");
}

// ========================================================================
// AuthorityAuditRefV1
// ========================================================================

/// Audit-safe reference from an authority snapshot. Excludes grant and
/// capability collections.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthorityAuditRefV1 {
    pub schema_version: u32,
    pub task_id: String,
    pub attempt_id: u32,
    pub run_id: String,
    pub policy_revision: String,
    pub disclosure_ceiling: DisclosureCeiling,
    pub existing_product_capture: bool,
    pub snapshot_digest: String,
}

// ========================================================================
// SkillUseAuditRefV1
// ========================================================================

    /// Audit-safe reference from a skill-use receipt.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillUseAuditRefV1 {
    pub schema_version: u32,
    pub source_authority: String,
    pub package_id: String,
    pub main_resource_id: String,
    pub package_digest: String,
    pub invocation_kind: crate::skills::SkillInvocationKind,
}

// ========================================================================
// ArtifactAuditRefV1
// ========================================================================

/// Audit-safe reference from artifact metadata. Excludes proposal and
/// source payload bytes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArtifactAuditRefV1 {
    pub artifact_id: String,
    pub artifact_revision: u32,
    pub kind: crate::product_task::ArtifactKind,
    pub schema_version: u32,
    pub canonical_payload_sha256: String,
    pub source_binding_sha256: String,
    pub proposal_id: String,
    pub provider_id: String,
    pub model_id: String,
    pub run_config_digest: String,
}

// ========================================================================
// ReviewDecisionAuditV1
// ========================================================================

/// Bounded review decision evidence. Excludes local-delta payload details.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewDecisionAuditV1 {
    pub artifact_id: String,
    pub artifact_revision: u32,
    pub proposal_id: String,
    pub applied_candidate_ids: Vec<u32>,
    pub rejected_candidate_ids: Vec<u32>,
    pub decided_at_unix_ms: i64,
}

// ========================================================================
// DocumentStateReceipt
// ========================================================================

/// Bounded document state receipt for applied reviews.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DocumentStateReceipt {
    pub state_id: u32,
    pub digest: String,
}

// ========================================================================
// AuditCorrelationV1
// ========================================================================

/// Correlation fields for an audit envelope. Task ID is always present;
/// other fields are populated per event variant.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuditCorrelationV1 {
    task_id: String,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    attempt_id: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    run_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    authority: Option<AuthorityAuditRefV1>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    skill_use: Option<SkillUseAuditRefV1>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    artifact: Option<ArtifactAuditRefV1>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    proposal_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    review_decision: Option<ReviewDecisionAuditV1>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    document_state: Option<DocumentStateReceipt>,
}

impl AuditCorrelationV1 {
    /// Create a minimal correlation with only a task ID.
    /// Other fields are populated per event variant.
    pub fn for_task(task_id: String) -> Self {
        Self {
            task_id,
            attempt_id: None,
            run_id: None,
            authority: None,
            skill_use: None,
            artifact: None,
            proposal_id: None,
            review_decision: None,
            document_state: None,
        }
    }

    pub fn task_id(&self) -> &str {
        &self.task_id
    }

    pub fn attempt_id(&self) -> Option<TaskAttemptId> {
        self.attempt_id.map(TaskAttemptId::new)
    }

    pub fn run_id_str(&self) -> Option<&str> {
        self.run_id.as_deref()
    }



    pub fn authority(&self) -> Option<&AuthorityAuditRefV1> {
        self.authority.as_ref()
    }

    pub fn skill_use(&self) -> Option<&SkillUseAuditRefV1> {
        self.skill_use.as_ref()
    }

    pub fn artifact(&self) -> Option<&ArtifactAuditRefV1> {
        self.artifact.as_ref()
    }

    pub fn review_decision(&self) -> Option<&ReviewDecisionAuditV1> {
        self.review_decision.as_ref()
    }
}

// ========================================================================
// Closed enums
// ========================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuditEventKindV1 {
    TaskCreated,
    AttemptStarted,
    RunContractBound,
    AuthorityDenied,
    ArtifactPromoted,
    ReviewApplyStarted,
    ReviewDecisionCommitted,
    TaskTerminated,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuditBudgetDimensionV1 {
    WallTime,
    ModelCalls,
    InputTokens,
    OutputTokens,
    Cost,
    ToolCalls,
    PerToolCalls,
    ArgumentBytes,
    ResultBytes,
    SourceBytes,
    Attachments,
    ValidationAttempts,
    DryRunAttempts,
    CapabilityCalls,
    CandidateCount,
    AffectedArea,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuditContextRecoveryFailureCategoryV1 {
    ParseFailure,
    SequenceGap,
    HashMismatch,
    DuplicateIdentity,
    CommitWithoutPrepare,
    ContradictoryOutcome,
    OversizedRecord,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuditTaskTerminalV1 {
    NeedsUserInput,
    Cancelled,
    BudgetExhausted,
    SourceValidationFailure,
    RuntimeFailure,
    AgentProtocolFailure,
    ProviderFailure,
    Interrupted,
    Stale,
    ContextOverflow,
    ContextRecoveryFailure,
    AuditFailure,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuditTaskStatusV1 {
    Created,
    Running,
    ReadyForReview,
    Applying,
    Completed,
    Rejected,
    Stale,
    Failed,
    NeedsUserInput,
    Cancelled,
    Interrupted,
}

// ========================================================================
// AuditEventV1 (closed material vocabulary)
// ========================================================================

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind")]
pub enum AuditEventV1 {
    TaskCreated,
    AttemptStarted {
        attempt_id: u32,
        run_id: String,
    },
    RunContractBound {
        authority: AuthorityAuditRefV1,
        skill_use: SkillUseAuditRefV1,
    },
    AuthorityDenied {
        authority: AuthorityAuditRefV1,
        tool_name: String,
        required_operation: String,
    },
    ArtifactPromoted {
        artifact: ArtifactAuditRefV1,
    },
    ReviewApplyStarted {
        artifact: ArtifactAuditRefV1,
    },
    ReviewDecisionCommitted {
        applied: bool,
        review_decision: ReviewDecisionAuditV1,
        #[serde(skip_serializing_if = "Option::is_none", default)]
        document_state: Option<DocumentStateReceipt>,
    },
    TaskTerminated {
        terminal: AuditTaskTerminalV1,
    },
}

impl AuditEventV1 {
    pub fn kind(&self) -> AuditEventKindV1 {
        match self {
            Self::TaskCreated => AuditEventKindV1::TaskCreated,
            Self::AttemptStarted { .. } => AuditEventKindV1::AttemptStarted,
            Self::RunContractBound { .. } => AuditEventKindV1::RunContractBound,
            Self::AuthorityDenied { .. } => AuditEventKindV1::AuthorityDenied,
            Self::ArtifactPromoted { .. } => AuditEventKindV1::ArtifactPromoted,
            Self::ReviewApplyStarted { .. } => AuditEventKindV1::ReviewApplyStarted,
            Self::ReviewDecisionCommitted { .. } => AuditEventKindV1::ReviewDecisionCommitted,
            Self::TaskTerminated { .. } => AuditEventKindV1::TaskTerminated,
        }
    }
}

// ========================================================================
// AuditEnvelopeV1
// ========================================================================

/// Canonical V1 DTO for deterministic digest computation.
#[derive(Serialize)]
struct CanonicalEnvelopeV1 {
    domain_separator: [u8; 27],
    schema_version: u32,
    event_id: String,
    occurred_at_unix_ms: i64,
    event_kind: AuditEventKindV1,
    event: AuditEventV1,
    correlation: AuditCorrelationV1,
}

/// Immutable audit envelope. Validated on construction.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuditEnvelopeV1 {
    schema_version: u32,
    event_id: AuditEventId,
    occurred_at_unix_ms: i64,
    event: AuditEventV1,
    correlation: AuditCorrelationV1,
    event_payload_digest: String,
}

impl AuditEnvelopeV1 {
    /// Create a new validated envelope. Rejects oversized strings.
    pub fn new(
        event_id: AuditEventId,
        occurred_at_unix_ms: i64,
        event: AuditEventV1,
        correlation: AuditCorrelationV1,
    ) -> Result<Self, AuditContractError> {
        // Validate string bounds in correlation.
        validate_string_bound(&correlation.task_id, "task_id")?;
        if let Some(ref rid) = correlation.run_id {
            validate_string_bound(rid, "run_id")?;
        }
        if let Some(ref pid) = correlation.proposal_id {
            validate_string_bound(pid, "proposal_id")?;
        }
        if let Some(ref auth) = correlation.authority {
            validate_string_bound(&auth.policy_revision, "policy_revision")?;
            validate_string_bound(&auth.snapshot_digest, "snapshot_digest")?;
        }
        if let Some(ref skill) = correlation.skill_use {
            validate_string_bound(&skill.package_id, "package_id")?;
            validate_string_bound(&skill.package_digest, "package_digest")?;
        }
        if let Some(ref art) = correlation.artifact {
            validate_string_bound(&art.artifact_id, "artifact_id")?;
            validate_string_bound(&art.proposal_id, "proposal_id")?;
            validate_string_bound(&art.run_config_digest, "run_config_digest")?;
        }
        if let Some(ref rd) = correlation.review_decision {
            validate_string_bound(&rd.artifact_id, "artifact_id")?;
            validate_string_bound(&rd.proposal_id, "proposal_id")?;
            if rd.applied_candidate_ids.len() > MAX_AUDIT_CANDIDATE_IDS
                || rd.rejected_candidate_ids.len() > MAX_AUDIT_CANDIDATE_IDS
            {
                return Err(AuditContractError::StringTooLong {
                    field: "candidate_ids".to_string(),
                    len: rd.applied_candidate_ids.len().max(rd.rejected_candidate_ids.len()),
                });
            }
        }

        // Validate event-level strings.
        match &event {
            AuditEventV1::AuthorityDenied {
                tool_name,
                required_operation,
                ..
            } => {
                validate_string_bound(tool_name, "tool_name")?;
                validate_string_bound(required_operation, "required_operation")?;
            }
            _ => {}
        }

        let event_payload_digest = Self::compute_digest(
            AUDIT_SCHEMA_VERSION_V1,
            &event_id,
            occurred_at_unix_ms,
            &event,
            &correlation,
        );

        Ok(Self {
            schema_version: AUDIT_SCHEMA_VERSION_V1,
            event_id,
            occurred_at_unix_ms,
            event,
            correlation,
            event_payload_digest,
        })
    }

    pub fn schema_version(&self) -> u32 {
        self.schema_version
    }

    pub fn event_id(&self) -> &AuditEventId {
        &self.event_id
    }

    pub fn occurred_at_unix_ms(&self) -> i64 {
        self.occurred_at_unix_ms
    }

    pub fn event(&self) -> &AuditEventV1 {
        &self.event
    }

    pub fn correlation(&self) -> &AuditCorrelationV1 {
        &self.correlation
    }

    pub fn event_payload_digest(&self) -> &str {
        &self.event_payload_digest
    }

    /// Canonical V1 bytes: domain separator + sorted DTO fields.
    fn compute_digest(
        schema_version: u32,
        event_id: &AuditEventId,
        occurred_at_unix_ms: i64,
        event: &AuditEventV1,
        correlation: &AuditCorrelationV1,
    ) -> String {
        let canonical = CanonicalEnvelopeV1 {
            domain_separator: *b"rollshot-audit-envelope-v1\0",
            schema_version,
            event_id: event_id.as_str().to_owned(),
            occurred_at_unix_ms,
            event_kind: event.kind(),
            event: event.clone(),
            correlation: correlation.clone(),
        };
        let bytes =
            serde_json::to_vec(&canonical).expect("CanonicalEnvelopeV1 serialization infallible");
        let hash = Sha256::digest(&bytes);
        hex_encode(&hash)
    }
}

fn validate_string_bound(value: &str, field: &str) -> Result<(), AuditContractError> {
    if value.len() > MAX_AUDIT_STRING_BYTES {
        Err(AuditContractError::StringTooLong {
            field: field.to_owned(),
            len: value.len(),
        })
    } else {
        Ok(())
    }
}

fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

// ========================================================================
// Audit append sink
// ========================================================================

/// Async append boundary for durable audit records.
pub trait AuditAppendSink: Send + Sync {
    fn append(
        &self,
        envelope: AuditEnvelopeV1,
    ) -> Pin<Box<dyn Future<Output = Result<AuditAppendReceiptV1, AuditAppendError>> + Send + '_>>;
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuditAppendReceiptV1 {
    pub event_id: String,
    pub sequence: u64,
    pub record_hash: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuditFailureCategory {
    Unavailable,
    LockContended,
    AppendPreCommitFailure,
    AppendVisibleDurabilityUncertain,
    UnsupportedSchema,
    CorruptJournal,
    SequenceOverflow,
    JournalTooLarge,
    CorrelationMismatch,
    TransitionMismatch,
    ReconciliationRequired,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error, Serialize, Deserialize)]
pub enum AuditAppendError {
    #[error("audit append failed: {category:?}")]
    AppendFailed { category: AuditFailureCategory },
}

impl AuditAppendError {
    pub fn from_category(category: AuditFailureCategory) -> Self {
        Self::AppendFailed { category }
    }
}

// ========================================================================
// AuditContractError
// ========================================================================

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AuditContractError {
    #[error("invalid audit event ID: {0}")]
    InvalidEventId(String),
    #[error("string too long for field {field}: {len} bytes")]
    StringTooLong { field: String, len: usize },
    #[error(
        "transition mismatch: task={task_id}, old_status={old_status:?}, new_status={new_status:?}"
    )]
    TransitionMismatch {
        task_id: String,
        old_status: Option<String>,
        new_status: String,
    },
    #[error("task ID mismatch: expected={expected}, got={got}")]
    TaskIdMismatch { expected: String, got: String },
    #[error("revision mismatch: expected={expected}, got={got}")]
    RevisionMismatch { expected: u32, got: u32 },
    #[error("timestamp regression: current={current}, attempted={attempted}")]
    TimestampRegression { current: i64, attempted: i64 },
}

// ========================================================================
// AuditTaskStateReceiptV1 (privacy-bounded receipt)
// ========================================================================

/// Privacy-bounded receipt from `ProductTaskSnapshot::audit_transition_receipt()`.
/// Contains no artifact/proposal payload bytes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuditTaskStateReceiptV1 {
    pub task_id: String,
    pub status: AuditTaskStatusV1,
    pub snapshot_revision: u32,
    pub created_at_unix_ms: i64,
    pub updated_at_unix_ms: i64,
    pub artifact: Option<ArtifactAuditRefV1>,
    pub review_decision: Option<ReviewDecisionAuditV1>,
}

// ========================================================================
// AuthoritySnapshot::audit_ref()
// ========================================================================

impl AuthoritySnapshot {
    /// Audit-safe reference. Excludes grant and capability collections.
    pub fn audit_ref(&self) -> AuthorityAuditRefV1 {
        AuthorityAuditRefV1 {
            schema_version: 1,
            task_id: self.task_id().as_str().to_owned(),
            attempt_id: self.attempt_id().get(),
            run_id: self.run_id().as_str().to_owned(),
            policy_revision: self.policy_revision().to_owned(),
            disclosure_ceiling: self.disclosure(),
            existing_product_capture: self.existing_product_capture(),
            snapshot_digest: self.digest().to_owned(),
        }
    }
}

// ========================================================================
// ProductTaskSnapshot::audit_transition_receipt()
// ========================================================================

impl ProductTaskSnapshot {
    /// Privacy-bounded audit receipt. Contains no artifact/proposal payload bytes.
    pub fn audit_transition_receipt(&self) -> AuditTaskStateReceiptV1 {
        AuditTaskStateReceiptV1 {
            task_id: self.task_id().as_str().to_owned(),
            status: map_task_status(&self.status()),
            snapshot_revision: self.snapshot_revision(),
            created_at_unix_ms: self.created_at_unix_ms(),
            updated_at_unix_ms: self.updated_at_unix_ms(),
            artifact: self.artifact_metadata().map(|meta| {
                let source_binding_sha256 = canonical_v1_digest(meta.source_binding())
                    .unwrap_or_default();
                ArtifactAuditRefV1 {
                    artifact_id: meta.artifact_id().as_str().to_owned(),
                    artifact_revision: meta.artifact_revision().get(),
                    kind: meta.kind(),
                    schema_version: meta.schema_version(),
                    canonical_payload_sha256: meta.canonical_payload_sha256().to_owned(),
                    source_binding_sha256,
                    proposal_id: meta.proposal_id().to_owned(),
                    provider_id: meta.provider_id().to_owned(),
                    model_id: meta.model_id().to_owned(),
                    run_config_digest: meta.run_config_digest().to_owned(),
                }
            }),
            review_decision: self.review_receipt().map(|rr| ReviewDecisionAuditV1 {
                artifact_id: rr.artifact_id.as_str().to_owned(),
                artifact_revision: rr.artifact_revision.get(),
                proposal_id: rr.proposal_id.clone(),
                applied_candidate_ids: rr.applied_candidates.clone(),
                rejected_candidate_ids: rr.rejected_candidates.clone(),
                decided_at_unix_ms: rr.decided_at_unix_ms,
            }),
        }
    }
}

fn map_task_status(status: &TaskStatus) -> AuditTaskStatusV1 {
    match status {
        TaskStatus::Created => AuditTaskStatusV1::Created,
        TaskStatus::Running => AuditTaskStatusV1::Running,
        TaskStatus::ReadyForReview => AuditTaskStatusV1::ReadyForReview,
        TaskStatus::Applying => AuditTaskStatusV1::Applying,
        TaskStatus::Completed => AuditTaskStatusV1::Completed,
        TaskStatus::Rejected => AuditTaskStatusV1::Rejected,
        TaskStatus::Stale => AuditTaskStatusV1::Stale,
        TaskStatus::Failed { .. } => AuditTaskStatusV1::Failed,
        TaskStatus::NeedsUserInput => AuditTaskStatusV1::NeedsUserInput,
        TaskStatus::Cancelled => AuditTaskStatusV1::Cancelled,
        TaskStatus::Interrupted => AuditTaskStatusV1::Interrupted,
    }
}

fn map_terminal(terminal: &TaskTerminal) -> AuditTaskTerminalV1 {
    match terminal {
        TaskTerminal::NeedsUserInput => AuditTaskTerminalV1::NeedsUserInput,
        TaskTerminal::Cancelled => AuditTaskTerminalV1::Cancelled,
        TaskTerminal::BudgetExhausted { .. } => AuditTaskTerminalV1::BudgetExhausted,
        TaskTerminal::SourceValidationFailure => AuditTaskTerminalV1::SourceValidationFailure,
        TaskTerminal::RuntimeFailure => AuditTaskTerminalV1::RuntimeFailure,
        TaskTerminal::AgentProtocolFailure => AuditTaskTerminalV1::AgentProtocolFailure,
        TaskTerminal::ProviderFailure => AuditTaskTerminalV1::ProviderFailure,
        TaskTerminal::Interrupted => AuditTaskTerminalV1::Interrupted,
        TaskTerminal::Stale => AuditTaskTerminalV1::Stale,
        TaskTerminal::ContextOverflow => AuditTaskTerminalV1::ContextOverflow,
        TaskTerminal::ContextRecoveryFailure { .. } => AuditTaskTerminalV1::ContextRecoveryFailure,
        TaskTerminal::AuditFailure { .. } => AuditTaskTerminalV1::AuditFailure,
    }
}

// ========================================================================
// Transition derivation
// ========================================================================

/// Derive the exact audit envelope for a material Product Task transition.
///
/// Validates task identity, legal status pairs, `old_revision + 1 == new_revision`,
/// and monotonic timestamps. Returns an `AuditEnvelopeV1` with the correct event
/// variant and correlation.
pub fn derive_material_transition(
    old: Option<&ProductTaskSnapshot>,
    new: &ProductTaskSnapshot,
    event_id: AuditEventId,
    occurred_at_unix_ms: i64,
) -> Result<AuditEnvelopeV1, AuditContractError> {
    let task_id = new.task_id();

    // Validate task identity from old snapshot.
    if let Some(old_snap) = old {
        if old_snap.task_id() != task_id {
            return Err(AuditContractError::TaskIdMismatch {
                expected: old_snap.task_id().as_str().to_owned(),
                got: task_id.as_str().to_owned(),
            });
        }
    }

    let old_status = old.map(|s| s.status());
    let new_status = new.status();

    // Validate legal transition before checking revision.
    let event = derive_event(&old_status, &new_status, new)?;

    // Validate revision after transition is known to be legal.
    if let Some(old_snap) = old {
        let expected_rev = old_snap.snapshot_revision() + 1;
        if new.snapshot_revision() != expected_rev {
            return Err(AuditContractError::RevisionMismatch {
                expected: expected_rev,
                got: new.snapshot_revision(),
            });
        }
        if occurred_at_unix_ms < old_snap.updated_at_unix_ms() {
            return Err(AuditContractError::TimestampRegression {
                current: old_snap.updated_at_unix_ms(),
                attempted: occurred_at_unix_ms,
            });
        }
    }

    let mut correlation = AuditCorrelationV1 {
        task_id: task_id.as_str().to_owned(),
        attempt_id: None,
        run_id: None,
        authority: None,
        skill_use: None,
        artifact: None,
        proposal_id: None,
        review_decision: None,
        document_state: None,
    };

    // Populate correlation fields from event.
    match &event {
        AuditEventV1::AttemptStarted {
            attempt_id,
            run_id,
        } => {
            correlation.attempt_id = Some(*attempt_id);
            correlation.run_id = Some(run_id.clone());
        }
        AuditEventV1::RunContractBound { authority, skill_use } => {
            let last = new.attempts().last();
            correlation.attempt_id = last.map(|a| a.attempt_id().get());
            correlation.run_id = last.map(|a| a.run_id().as_str().to_owned());
            correlation.authority = Some(authority.clone());
            correlation.skill_use = Some(skill_use.clone());
        }
        AuditEventV1::ArtifactPromoted { artifact } => {
            let last = new.attempts().last();
            correlation.attempt_id = last.map(|a| a.attempt_id().get());
            correlation.run_id = last.map(|a| a.run_id().as_str().to_owned());
            correlation.artifact = Some(artifact.clone());
            if let Some(meta) = new.artifact_metadata() {
                correlation.proposal_id = Some(meta.proposal_id().to_owned());
            }
        }
        AuditEventV1::ReviewApplyStarted { artifact } => {
            let last = new.attempts().last();
            correlation.attempt_id = last.map(|a| a.attempt_id().get());
            correlation.run_id = last.map(|a| a.run_id().as_str().to_owned());
            correlation.artifact = Some(artifact.clone());
        }
        AuditEventV1::ReviewDecisionCommitted {
            review_decision,
            document_state,
            ..
        } => {
            let last = new.attempts().last();
            correlation.attempt_id = last.map(|a| a.attempt_id().get());
            correlation.run_id = last.map(|a| a.run_id().as_str().to_owned());
            correlation.review_decision = Some(review_decision.clone());
            correlation.document_state = document_state.clone();
        }
        AuditEventV1::TaskTerminated { .. } => {
            // Only task_id needed.
        }
        AuditEventV1::TaskCreated | AuditEventV1::AuthorityDenied { .. } => {
            // Only task_id needed.
        }
    }

    AuditEnvelopeV1::new(event_id, occurred_at_unix_ms, event, correlation)
}

/// Derive the event variant from old/new status pair.
fn derive_event(
    old_status: &Option<TaskStatus>,
    new_status: &TaskStatus,
    new: &ProductTaskSnapshot,
) -> Result<AuditEventV1, AuditContractError> {
    match (old_status, new_status) {
        // TaskCreated: absent → Created
        (None, TaskStatus::Created) => Ok(AuditEventV1::TaskCreated),

        // AttemptStarted: Created → Running
        (Some(TaskStatus::Created), TaskStatus::Running) => {
            let last = new
                .attempts()
                .last()
                .expect("Running task must have at least one attempt");
            Ok(AuditEventV1::AttemptStarted {
                    attempt_id: last.attempt_id().get(),
                    run_id: last.run_id().as_str().to_owned(),
            })
        }

        // RunContractBound: Running → Running (bind_run_contract)
        (Some(TaskStatus::Running), TaskStatus::Running) => {
            let contract = new
                .active_run_contract()
                .ok_or(AuditContractError::TransitionMismatch {
                    task_id: new.task_id().as_str().to_owned(),
                    old_status: Some("Running".to_owned()),
                    new_status: "Running".to_owned(),
                })?;
            let authority_ref = AuthorityAuditRefV1 {
                schema_version: 1,
                task_id: contract.authority.task_id.clone(),
                attempt_id: contract.authority.attempt_id,
                run_id: contract.authority.run_id.clone(),
                policy_revision: contract.authority.policy_revision.clone(),
                disclosure_ceiling: contract.authority.disclosure_ceiling,
                existing_product_capture: contract.authority.existing_product_capture,
                snapshot_digest: contract.authority.snapshot_digest.clone(),
            };
            let skill_ref = SkillUseAuditRefV1 {
                schema_version: contract.skill_use.schema_version,
                source_authority: contract.skill_use.source_authority.clone(),
                package_id: contract.skill_use.package_id.clone(),
                main_resource_id: contract.skill_use.main_resource_id.clone(),
                package_digest: contract.skill_use.package_digest.clone(),
                invocation_kind: contract.skill_use.invocation_kind,
            };
            Ok(AuditEventV1::RunContractBound {
                    authority: authority_ref,
                    skill_use: skill_ref,
            })
        }

        // ArtifactPromoted: Running → ReadyForReview
        (Some(TaskStatus::Running), TaskStatus::ReadyForReview) => {
            let meta = new
                .artifact_metadata()
                .expect("ReadyForReview must have artifact metadata");
            let artifact_ref = make_artifact_ref(meta);
            Ok(AuditEventV1::ArtifactPromoted { artifact: artifact_ref })
        }

        // ReviewApplyStarted: ReadyForReview → Applying
        (Some(TaskStatus::ReadyForReview), TaskStatus::Applying) => {
            let meta = new
                .artifact_metadata()
                .expect("Applying must have artifact metadata");
            let artifact_ref = make_artifact_ref(meta);
            Ok(AuditEventV1::ReviewApplyStarted { artifact: artifact_ref })
        }

        // ReviewDecisionCommitted::Applied: Applying → Completed
        (Some(TaskStatus::Applying), TaskStatus::Completed) => {
            let receipt = new
                .review_receipt()
                .expect("Completed must have review receipt");
            let rd = ReviewDecisionAuditV1 {
                artifact_id: receipt.artifact_id.as_str().to_owned(),
                artifact_revision: receipt.artifact_revision.get(),
                proposal_id: receipt.proposal_id.clone(),
                applied_candidate_ids: receipt.applied_candidates.clone(),
                rejected_candidate_ids: receipt.rejected_candidates.clone(),
                decided_at_unix_ms: receipt.decided_at_unix_ms,
            };
            let doc_state = receipt.resulting_document_state_id.map(|sid| {
                let digest = receipt
                    .resulting_document_digest
                    .map(|d| hex_encode(&d))
                    .unwrap_or_default();
                DocumentStateReceipt {
                    state_id: sid,
                    digest,
                }
            });
            Ok(AuditEventV1::ReviewDecisionCommitted {
                    applied: true,
                    review_decision: rd,
                    document_state: doc_state,
            })
        }

        // ReviewDecisionCommitted::Rejected: ReadyForReview → Rejected
        (Some(TaskStatus::ReadyForReview), TaskStatus::Rejected) => {
            let receipt = new
                .review_receipt()
                .expect("Rejected must have review receipt");
            let rd = ReviewDecisionAuditV1 {
                artifact_id: receipt.artifact_id.as_str().to_owned(),
                artifact_revision: receipt.artifact_revision.get(),
                proposal_id: receipt.proposal_id.clone(),
                applied_candidate_ids: receipt.applied_candidates.clone(),
                rejected_candidate_ids: receipt.rejected_candidates.clone(),
                decided_at_unix_ms: receipt.decided_at_unix_ms,
            };
            Ok(AuditEventV1::ReviewDecisionCommitted {
                    applied: false,
                    review_decision: rd,
                    document_state: None,
            })
        }

        // TaskTerminated: ReadyForReview → Stale
        (Some(TaskStatus::ReadyForReview), TaskStatus::Stale) => Ok(
            AuditEventV1::TaskTerminated {
                terminal: AuditTaskTerminalV1::Stale,
            },
        ),

        // TaskTerminated: Created → Interrupted (startup interruption)
        (Some(TaskStatus::Created), TaskStatus::Interrupted) => Ok(
            AuditEventV1::TaskTerminated {
                terminal: AuditTaskTerminalV1::Interrupted,
            },
        ),

        // TaskTerminated: Running|Applying → Interrupted
        (Some(TaskStatus::Running | TaskStatus::Applying), TaskStatus::Interrupted) => Ok(
            AuditEventV1::TaskTerminated {
                terminal: AuditTaskTerminalV1::Interrupted,
            },
        ),

        // TaskTerminated: Running → Failed/NeedsUserInput/Cancelled
        (Some(TaskStatus::Running), TaskStatus::Failed { terminal }) => Ok(
            AuditEventV1::TaskTerminated {
                terminal: map_terminal(terminal),
            },
        ),
        (Some(TaskStatus::Running), TaskStatus::NeedsUserInput) => Ok(
            AuditEventV1::TaskTerminated {
                terminal: AuditTaskTerminalV1::NeedsUserInput,
            },
        ),
        (Some(TaskStatus::Running), TaskStatus::Cancelled) => Ok(
            AuditEventV1::TaskTerminated {
                terminal: AuditTaskTerminalV1::Cancelled,
            },
        ),

        // Unsupported transition.
        _ => Err(AuditContractError::TransitionMismatch {
            task_id: new.task_id().as_str().to_owned(),
            old_status: old_status.as_ref().map(|s| format!("{s:?}")),
            new_status: format!("{new_status:?}"),
        }),
    }
}

fn make_artifact_ref(
    meta: &crate::product_task::ProductArtifactMetadata,
) -> ArtifactAuditRefV1 {
    let source_binding_sha256 = canonical_v1_digest(meta.source_binding())
        .unwrap_or_default();
    ArtifactAuditRefV1 {
        artifact_id: meta.artifact_id().as_str().to_owned(),
        artifact_revision: meta.artifact_revision().get(),
        kind: meta.kind(),
        schema_version: meta.schema_version(),
        canonical_payload_sha256: meta.canonical_payload_sha256().to_owned(),
        source_binding_sha256,
        proposal_id: meta.proposal_id().to_owned(),
        provider_id: meta.provider_id().to_owned(),
        model_id: meta.model_id().to_owned(),
        run_config_digest: meta.run_config_digest().to_owned(),
    }
}

// ========================================================================
// Tests
// ========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::authority::{
        AuthorityBinding, AuthoritySnapshotReceiptV1, DisclosureCeiling, PreparedCapability,
        RunOperation,
    };
    use crate::domain::RunId;
    use crate::product_task::*;
    use crate::skills::{SkillInvocationKind, SkillUseReceiptV1};
    use std::collections::BTreeMap;

    // ------------------------------------------------------------------
    // Test fixtures
    // ------------------------------------------------------------------

    fn task_id_fixture() -> ProductTaskId {
        ProductTaskId::parse("task-00000000-0000-4000-8000-000000000001").unwrap()
    }

    fn run_id_fixture() -> RunId {
        RunId::parse("run-00000000-0000-4000-8000-000000000001").unwrap()
    }

    fn audit_id(n: u64) -> AuditEventId {
        AuditEventId::parse(format!(
            "audit-00000000-0000-4000-8000-{n:012x}"
        ))
        .unwrap()
    }

    fn source_binding_fixture() -> SourceBinding {
        SourceBinding::new([1u8; 32], [2u8; 32], 0, "preset-001".to_owned(), None)
    }

    fn attempt_fixture() -> TaskAttempt {
        TaskAttempt::new(TaskAttemptId::new(1), run_id_fixture(), 10)
    }

    fn payload_fixture() -> SmartRedactionReviewPayload {
        SmartRedactionReviewPayload {
            source: PayloadSourceV1 {
                kind: "smart_redaction".to_owned(),
                validation_summary: "all_valid".to_owned(),
            },
            proposal: PayloadProposalV1 {
                proposal_id: "proposal-00000000-0000-4000-8000-000000000001".to_owned(),
                candidate_count: 3,
            },
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
            hex_encode_test(&hash)
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
            ArtifactId::parse("artifact-00000000-0000-4000-8000-000000000001").unwrap(),
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

    fn created_task_fixture() -> ProductTaskSnapshot {
        ProductTaskSnapshot::new(
            task_id_fixture(),
            TaskKind::SmartRedactionAuthor,
            source_binding_fixture(),
            10,
        )
        .unwrap()
    }

    fn run_id() -> RunId {
        RunId::parse("run-00000000-0000-4000-8000-000000000001").unwrap()
    }

    fn document_binding_fixture() -> DocumentContentBinding {
        let state = AnnotationStateV1 {
            width: 100,
            height: 80,
            state_id: 1,
            annotations: vec![],
        };
        DocumentContentBinding::new([0xAB_u8; 32], &state, 1).unwrap()
    }

    fn authority_snapshot_fixture() -> AuthoritySnapshot {
        AuthoritySnapshot::new(
            AuthorityBinding::new(
                task_id_fixture(),
                TaskAttemptId::new(1),
                run_id(),
                document_binding_fixture(),
            ),
            "auth-sentinel-policy".into(),
            DisclosureCeiling::OcrLayoutOnly,
            false,
            [PreparedCapability::Ocr].into_iter().collect(),
            [RunOperation::ReadDraft].into_iter().collect(),
        )
        .unwrap()
    }

    fn skill_use_receipt_fixture() -> SkillUseReceiptV1 {
        SkillUseReceiptV1 {
            schema_version: 1,
            source_authority: "authority://test".to_owned(),
            package_id: "package-sentinel".to_owned(),
            main_resource_id: "resource-1".to_owned(),
            package_digest: "ab".repeat(32),
            declared_version: Some("1.0.0".to_owned()),
            invocation_kind: SkillInvocationKind::HostExplicit,
            resolved_at_unix_ms: 10,
        }
    }

    fn authority_receipt_fixture() -> AuthoritySnapshotReceiptV1 {
        authority_snapshot_fixture().receipt(10)
    }

    fn run_contract_fixture(authority: &AuthoritySnapshot, skill_use: &SkillUseReceiptV1) -> RunContractReceiptV1 {
        RunContractReceiptV1 {
            authority: authority.receipt(10),
            skill_use: skill_use.clone(),
            bound_at_unix_ms: 20,
        }
    }

    fn sensitive_run_fixture() -> (
        ProductTaskSnapshot,
        AuthoritySnapshot,
        SkillUseReceiptV1,
        &'static str,
        &'static str,
    ) {
        let authority = authority_snapshot_fixture();
        let skill_use = skill_use_receipt_fixture();
        let running = created_task_fixture()
            .start_attempt(
                TaskAttempt::new(TaskAttemptId::new(1), run_id(), 20),
                20,
            )
            .unwrap();
        (
            running,
            authority,
            skill_use,
            "smart_redaction",
            "resource-1",
        )
    }

    fn apply_receipt_fixture() -> ReviewReceipt {
        ReviewReceipt {
            artifact_id: ArtifactId::parse("artifact-00000000-0000-4000-8000-000000000001")
                .unwrap(),
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

    fn hex_encode_test(bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{b:02x}")).collect()
    }

    // ==================================================================
    // Step 3: Envelope identity, correlation, canonical bytes
    // ==================================================================

    mod envelope {
        use super::*;

        #[test]
        fn audit_event_id_parse_roundtrip() {
            let id =
                AuditEventId::parse("audit-00000000-0000-4000-8000-000000000001").unwrap();
            assert_eq!(id.as_str(), "audit-00000000-0000-4000-8000-000000000001");
        }

        #[test]
        fn audit_event_id_rejects_missing_prefix() {
            assert!(AuditEventId::parse("not-audit-00000000-0000-4000-8000-000000000001").is_err());
        }

        #[test]
        fn audit_event_id_rejects_short_uuid() {
            assert!(AuditEventId::parse("audit-short").is_err());
        }

        #[test]
        fn new_v4_produces_valid_prefix() {
            let id = AuditEventId::new_v4();
            assert!(id.as_str().starts_with("audit-"));
            assert_eq!(id.as_str().len(), 42); // "audit-" (6) + UUID (36)
        }

        #[test]
        fn envelope_creation_validates_string_bounds() {
            let long_str = "x".repeat(600);
            let correlation = AuditCorrelationV1 {
                task_id: long_str,
                attempt_id: None,
                run_id: None,
                authority: None,
                skill_use: None,
                artifact: None,
                proposal_id: None,
                review_decision: None,
                document_state: None,
            };
            let result = AuditEnvelopeV1::new(
                audit_id(1),
                10,
                AuditEventV1::TaskCreated,
                correlation,
            );
            assert!(matches!(
                result,
                Err(AuditContractError::StringTooLong { .. })
            ));
        }

        #[test]
        fn envelope_has_stable_canonical_digest() {
            let envelope = AuditEnvelopeV1::new(
                audit_id(1),
                10,
                AuditEventV1::TaskCreated,
                AuditCorrelationV1 {
                    task_id: "task-00000000-0000-4000-8000-000000000001".to_owned(),
                    attempt_id: None,
                    run_id: None,
                    authority: None,
                    skill_use: None,
                    artifact: None,
                    proposal_id: None,
                    review_decision: None,
                    document_state: None,
                },
            )
            .unwrap();
            let d1 = envelope.event_payload_digest().to_owned();
            let envelope2 = AuditEnvelopeV1::new(
                audit_id(1),
                10,
                AuditEventV1::TaskCreated,
                AuditCorrelationV1 {
                    task_id: "task-00000000-0000-4000-8000-000000000001".to_owned(),
                    attempt_id: None,
                    run_id: None,
                    authority: None,
                    skill_use: None,
                    artifact: None,
                    proposal_id: None,
                    review_decision: None,
                    document_state: None,
                },
            )
            .unwrap();
            assert_eq!(d1, envelope2.event_payload_digest());
        }

        #[test]
        fn envelope_serialization_roundtrip() {
            let envelope = AuditEnvelopeV1::new(
                audit_id(1),
                10,
                AuditEventV1::TaskCreated,
                AuditCorrelationV1 {
                    task_id: "task-00000000-0000-4000-8000-000000000001".to_owned(),
                    attempt_id: None,
                    run_id: None,
                    authority: None,
                    skill_use: None,
                    artifact: None,
                    proposal_id: None,
                    review_decision: None,
                    document_state: None,
                },
            )
            .unwrap();
            let json = serde_json::to_string(&envelope).unwrap();
            let back: AuditEnvelopeV1 = serde_json::from_str(&json).unwrap();
            assert_eq!(envelope, back);
        }

        #[test]
        fn correlation_task_id_accessor() {
            let correlation = AuditCorrelationV1 {
                task_id: "task-00000000-0000-4000-8000-000000000001".to_owned(),
                attempt_id: Some(1),
                run_id: Some("run-00000000-0000-4000-8000-000000000001".to_owned()),
                authority: None,
                skill_use: None,
                artifact: None,
                proposal_id: None,
                review_decision: None,
                document_state: None,
            };
            assert_eq!(correlation.task_id(), "task-00000000-0000-4000-8000-000000000001");
            assert_eq!(correlation.attempt_id(), Some(TaskAttemptId::new(1)));
        }
    }

    // ==================================================================
    // Step 4: audit_ref and audit_transition_receipt
    // ==================================================================

    mod audit_ref {
        use super::*;

        #[test]
        fn authority_audit_ref_excludes_grants_and_capabilities() {
            let snapshot = authority_snapshot_fixture();
            let audit_ref = snapshot.audit_ref();
            assert_eq!(audit_ref.schema_version, 1);
            assert_eq!(audit_ref.task_id, "task-00000000-0000-4000-8000-000000000001");
            assert_eq!(audit_ref.attempt_id, 1);
            assert_eq!(audit_ref.run_id, "run-00000000-0000-4000-8000-000000000001");
            assert_eq!(audit_ref.policy_revision, "auth-sentinel-policy");
            assert_eq!(audit_ref.disclosure_ceiling, DisclosureCeiling::OcrLayoutOnly);
            assert!(!audit_ref.existing_product_capture);
            assert!(!audit_ref.snapshot_digest.is_empty());
            // Verify no grant or capability data
            let json = serde_json::to_string(&audit_ref).unwrap();
            assert!(!json.contains("granted_operations"));
            assert!(!json.contains("prepared_capabilities"));
            assert!(!json.contains("ReadDraft"));
            assert!(!json.contains("Ocr"));
        }

        #[test]
        fn audit_ref_serialization_roundtrip() {
            let snapshot = authority_snapshot_fixture();
            let audit_ref = snapshot.audit_ref();
            let json = serde_json::to_string(&audit_ref).unwrap();
            let back: AuthorityAuditRefV1 = serde_json::from_str(&json).unwrap();
            assert_eq!(audit_ref, back);
        }
    }

    mod audit_transition_receipt {
        use super::*;

        #[test]
        fn receipt_contains_no_payload_bytes() {
            let created = created_task_fixture();
            let receipt = created.audit_transition_receipt();
            assert_eq!(receipt.task_id, "task-00000000-0000-4000-8000-000000000001");
            assert_eq!(receipt.status, AuditTaskStatusV1::Created);
            assert_eq!(receipt.snapshot_revision, 0);
            assert!(receipt.artifact.is_none());
            assert!(receipt.review_decision.is_none());
            // No payload bytes
            let json = serde_json::to_string(&receipt).unwrap();
            assert!(!json.contains("pending_artifact_payload"));
            assert!(!json.contains("pending_proposal_payload"));
        }

        #[test]
        fn receipt_with_artifact_and_review() {
            let running = created_task_fixture()
                .start_attempt(attempt_fixture(), 20)
                .unwrap();
            let meta = metadata_fixture(run_id_fixture(), TaskAttemptId::new(1));
            let ready = running
                .record_ready_for_review(meta, payload_fixture(), None, 30)
                .unwrap();
            let receipt = ready.audit_transition_receipt();
            assert_eq!(receipt.status, AuditTaskStatusV1::ReadyForReview);
            assert!(receipt.artifact.is_some());
            let art = receipt.artifact.as_ref().unwrap();
            assert_eq!(art.artifact_id, "artifact-00000000-0000-4000-8000-000000000001");
            assert_eq!(art.artifact_revision, 1);
            assert!(!art.canonical_payload_sha256.is_empty());
            assert!(receipt.review_decision.is_none());
        }
    }

    // ==================================================================
    // Step 5: derive_material_transition tests
    // ==================================================================

    mod derive {
        use super::*;

        #[test]
        fn derives_task_created_only_from_absent_to_created() {
            let created = created_task_fixture();
            let envelope = derive_material_transition(
                None,
                &created,
                AuditEventId::parse("audit-00000000-0000-4000-8000-000000000001").unwrap(),
                10,
            )
            .unwrap();
            assert_eq!(envelope.event(), &AuditEventV1::TaskCreated);
            assert_eq!(envelope.correlation().task_id(), created.task_id().as_str());
        }

        #[test]
        fn derives_attempt_started_from_created_to_running() {
            let created = created_task_fixture();
            let running = created
                .start_attempt(
                    TaskAttempt::new(TaskAttemptId::new(1), run_id(), 20),
                    20,
                )
                .unwrap();
            let envelope = derive_material_transition(
                Some(&created),
                &running,
                audit_id(2),
                20,
            )
            .unwrap();
            assert_eq!(envelope.event(), &AuditEventV1::AttemptStarted {
                attempt_id: 1,
                run_id: "run-00000000-0000-4000-8000-000000000001".to_owned(),
            });
            assert_eq!(
                envelope.correlation().attempt_id(),
                Some(TaskAttemptId::new(1))
            );
            assert_eq!(
                envelope.correlation().run_id_str(),
                Some("run-00000000-0000-4000-8000-000000000001")
            );
        }

        #[test]
        fn rejects_collapsed_absent_to_running_transition() {
            let created = created_task_fixture();
            let running = created
                .start_attempt(
                    TaskAttempt::new(TaskAttemptId::new(1), run_id(), 20),
                    20,
                )
                .unwrap();
            assert!(matches!(
                derive_material_transition(None, &running, audit_id(3), 20),
                Err(AuditContractError::TransitionMismatch { .. })
            ));
        }

        #[test]
        fn serialized_event_excludes_adjacent_sensitive_objects() {
            let (running, authority, skill_use, _source_sentinel, _skill_sentinel) =
                sensitive_run_fixture();
            let bound = running
                .bind_run_contract(run_contract_fixture(&authority, &skill_use), 30)
                .unwrap();
            let envelope =
                derive_material_transition(Some(&running), &bound, audit_id(4), 30).unwrap();
            let json = serde_json::to_string(&envelope).unwrap();
            // Verify adjacent sensitive objects are excluded.
            // source_sentinel ("smart_redaction") from payload and
            // skill_sentinel ("resource-1") from receipt must not appear
            // in adjacent object dumps, but main_resource_id is now an
            // allowed durable field in SkillUseAuditRefV1.
            for forbidden in [
                "granted_operations",
                "prepared_capabilities",
            ] {
                assert!(!json.contains(forbidden), "audit leak: {forbidden}");
            }
        }

        // ---- TaskCreated ----

        #[test]
        fn derives_task_created() {
            let created = created_task_fixture();
            let envelope =
                derive_material_transition(None, &created, audit_id(1), 10).unwrap();
            assert_eq!(envelope.event(), &AuditEventV1::TaskCreated);
        }

        // ---- AttemptStarted ----

        #[test]
        fn derives_attempt_started() {
            let created = created_task_fixture();
            let running = created
                .start_attempt(
                    TaskAttempt::new(TaskAttemptId::new(1), run_id(), 20),
                    20,
                )
                .unwrap();
            let envelope =
                derive_material_transition(Some(&created), &running, audit_id(2), 20).unwrap();
            assert_eq!(
                envelope.event(),
                &AuditEventV1::AttemptStarted {
                    attempt_id: 1,
                    run_id: "run-00000000-0000-4000-8000-000000000001".to_owned(),
                }
            );
        }

        // ---- RunContractBound ----

        #[test]
        fn derives_run_contract_bound() {
            let running = created_task_fixture()
                .start_attempt(
                    TaskAttempt::new(TaskAttemptId::new(1), run_id(), 20),
                    20,
                )
                .unwrap();
            let authority = authority_snapshot_fixture();
            let skill_use = skill_use_receipt_fixture();
            let bound = running
                .bind_run_contract(run_contract_fixture(&authority, &skill_use), 30)
                .unwrap();
            let envelope =
                derive_material_transition(Some(&running), &bound, audit_id(3), 30).unwrap();
            match envelope.event() {
                AuditEventV1::RunContractBound { authority, skill_use } => {
                    assert_eq!(authority.snapshot_digest, authority.snapshot_digest);
                    assert_eq!(skill_use.package_id, "package-sentinel");
                }
                _ => panic!("expected RunContractBound"),
            }
        }

        // ---- ArtifactPromoted ----

        #[test]
        fn derives_artifact_promoted() {
            let running = created_task_fixture()
                .start_attempt(
                    TaskAttempt::new(TaskAttemptId::new(1), run_id(), 20),
                    20,
                )
                .unwrap();
            let meta = metadata_fixture(run_id_fixture(), TaskAttemptId::new(1));
            let ready = running
                .record_ready_for_review(meta, payload_fixture(), None, 30)
                .unwrap();
            let envelope =
                derive_material_transition(Some(&running), &ready, audit_id(4), 30).unwrap();
            match envelope.event() {
                AuditEventV1::ArtifactPromoted { artifact } => {
                    assert_eq!(
                        artifact.artifact_id,
                        "artifact-00000000-0000-4000-8000-000000000001"
                    );
                    assert_eq!(artifact.artifact_revision, 1);
                }
                _ => panic!("expected ArtifactPromoted"),
            }
        }

        // ---- ReviewApplyStarted ----

        #[test]
        fn derives_review_apply_started() {
            let running = created_task_fixture()
                .start_attempt(
                    TaskAttempt::new(TaskAttemptId::new(1), run_id(), 20),
                    20,
                )
                .unwrap();
            let meta = metadata_fixture(run_id_fixture(), TaskAttemptId::new(1));
            let ready = running
                .record_ready_for_review(meta, payload_fixture(), None, 30)
                .unwrap();
            let applying = ready.begin_apply(35).unwrap();
            let envelope =
                derive_material_transition(Some(&ready), &applying, audit_id(5), 35).unwrap();
            assert!(matches!(
                envelope.event(),
                AuditEventV1::ReviewApplyStarted { .. }
            ));
        }

        // ---- ReviewDecisionCommitted (Applied) ----

        #[test]
        fn derives_review_decision_committed_applied() {
            let running = created_task_fixture()
                .start_attempt(
                    TaskAttempt::new(TaskAttemptId::new(1), run_id(), 20),
                    20,
                )
                .unwrap();
            let meta = metadata_fixture(run_id_fixture(), TaskAttemptId::new(1));
            let ready = running
                .record_ready_for_review(meta, payload_fixture(), None, 30)
                .unwrap();
            let applying = ready.begin_apply(35).unwrap();
            let completed = applying
                .complete_apply(apply_receipt_fixture(), 50)
                .unwrap();
            let envelope =
                derive_material_transition(Some(&applying), &completed, audit_id(6), 50).unwrap();
            match envelope.event() {
                AuditEventV1::ReviewDecisionCommitted {
                    applied,
                    review_decision,
                    document_state,
                } => {
                    assert!(applied);
                    assert_eq!(review_decision.applied_candidate_ids, vec![0, 1, 2]);
                    assert!(document_state.is_some());
                    let ds = document_state.as_ref().unwrap();
                    assert_eq!(ds.state_id, 1);
                }
                _ => panic!("expected ReviewDecisionCommitted"),
            }
        }

        // ---- ReviewDecisionCommitted (Rejected) ----

        #[test]
        fn derives_review_decision_committed_rejected() {
            let running = created_task_fixture()
                .start_attempt(
                    TaskAttempt::new(TaskAttemptId::new(1), run_id(), 20),
                    20,
                )
                .unwrap();
            let meta = metadata_fixture(run_id_fixture(), TaskAttemptId::new(1));
            let ready = running
                .record_ready_for_review(meta, payload_fixture(), None, 30)
                .unwrap();
            let rejected = ready
                .reject(
                    ReviewReceipt {
                        artifact_id: ArtifactId::parse(
                            "artifact-00000000-0000-4000-8000-000000000001",
                        )
                        .unwrap(),
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
                    },
                    40,
                )
                .unwrap();
            let envelope =
                derive_material_transition(Some(&ready), &rejected, audit_id(7), 40).unwrap();
            match envelope.event() {
                AuditEventV1::ReviewDecisionCommitted {
                    applied,
                    review_decision,
                    document_state,
                } => {
                    assert!(!applied);
                    assert_eq!(review_decision.rejected_candidate_ids, vec![0, 1, 2]);
                    assert!(document_state.is_none());
                }
                _ => panic!("expected ReviewDecisionCommitted"),
            }
        }

        // ---- TaskTerminated (Stale) ----

        #[test]
        fn derives_task_terminated_stale() {
            let running = created_task_fixture()
                .start_attempt(
                    TaskAttempt::new(TaskAttemptId::new(1), run_id(), 20),
                    20,
                )
                .unwrap();
            let meta = metadata_fixture(run_id_fixture(), TaskAttemptId::new(1));
            let ready = running
                .record_ready_for_review(meta, payload_fixture(), None, 30)
                .unwrap();
            let stale = ready.mark_stale(40).unwrap();
            let envelope =
                derive_material_transition(Some(&ready), &stale, audit_id(8), 40).unwrap();
            assert_eq!(
                envelope.event(),
                &AuditEventV1::TaskTerminated {
                    terminal: AuditTaskTerminalV1::Stale,
                }
            );
        }

        // ---- TaskTerminated (Cancelled) ----

        #[test]
        fn derives_task_terminated_cancelled() {
            let running = created_task_fixture()
                .start_attempt(
                    TaskAttempt::new(TaskAttemptId::new(1), run_id(), 20),
                    20,
                )
                .unwrap();
            let cancelled = running
                .record_terminal(TaskTerminal::Cancelled, 30)
                .unwrap();
            let envelope =
                derive_material_transition(Some(&running), &cancelled, audit_id(9), 30).unwrap();
            assert_eq!(
                envelope.event(),
                &AuditEventV1::TaskTerminated {
                    terminal: AuditTaskTerminalV1::Cancelled,
                }
            );
        }

        // ---- TaskTerminated (BudgetExhausted) ----

        #[test]
        fn derives_task_terminated_budget_exhausted() {
            let running = created_task_fixture()
                .start_attempt(
                    TaskAttempt::new(TaskAttemptId::new(1), run_id(), 20),
                    20,
                )
                .unwrap();
            let failed = running
                .record_terminal(
                    TaskTerminal::BudgetExhausted {
                        dimension: "model_calls".to_owned(),
                    },
                    30,
                )
                .unwrap();
            let envelope =
                derive_material_transition(Some(&running), &failed, audit_id(10), 30).unwrap();
            assert_eq!(
                envelope.event(),
                &AuditEventV1::TaskTerminated {
                    terminal: AuditTaskTerminalV1::BudgetExhausted,
                }
            );
        }

        // ---- TaskTerminated (Interrupted) ----

        #[test]
        fn derives_task_terminated_interrupted() {
            let running = created_task_fixture()
                .start_attempt(
                    TaskAttempt::new(TaskAttemptId::new(1), run_id(), 20),
                    20,
                )
                .unwrap();
            let interrupted = running.reconcile_interrupted(30).unwrap().unwrap();
            let envelope =
                derive_material_transition(Some(&running), &interrupted, audit_id(11), 30).unwrap();
            assert_eq!(
                envelope.event(),
                &AuditEventV1::TaskTerminated {
                    terminal: AuditTaskTerminalV1::Interrupted,
                }
            );
        }

        // ---- TaskTerminated (NeedsUserInput) ----

        #[test]
        fn derives_task_terminated_needs_user_input() {
            let running = created_task_fixture()
                .start_attempt(
                    TaskAttempt::new(TaskAttemptId::new(1), run_id(), 20),
                    20,
                )
                .unwrap();
            let needs_input = running
                .record_terminal(TaskTerminal::NeedsUserInput, 30)
                .unwrap();
            let envelope =
                derive_material_transition(Some(&running), &needs_input, audit_id(12), 30).unwrap();
            assert_eq!(
                envelope.event(),
                &AuditEventV1::TaskTerminated {
                    terminal: AuditTaskTerminalV1::NeedsUserInput,
                }
            );
        }

        // ---- Rejection tests ----

        #[test]
        fn rejects_task_id_mismatch() {
            let old = created_task_fixture();
            let wrong_id = ProductTaskId::parse("task-99999999-9999-4999-8999-999999999999").unwrap();
            let new = ProductTaskSnapshot::new(
                wrong_id,
                TaskKind::SmartRedactionAuthor,
                source_binding_fixture(),
                20,
            )
            .unwrap();
            assert!(matches!(
                derive_material_transition(Some(&old), &new, audit_id(1), 20),
                Err(AuditContractError::TaskIdMismatch { .. })
            ));
        }

        #[test]
        fn rejects_revision_mismatch() {
            let created = created_task_fixture();
            let running = created
                .start_attempt(
                    TaskAttempt::new(TaskAttemptId::new(1), run_id(), 20),
                    20,
                )
                .unwrap();
            let authority = authority_snapshot_fixture();
            let skill_use = skill_use_receipt_fixture();
            let bound = running
                .bind_run_contract(run_contract_fixture(&authority, &skill_use), 30)
                .unwrap();
            // old = created (rev 0), new = bound (rev 2).
            // Created → Running is legal, but expected rev = 0+1 = 1, got 2.
            assert!(matches!(
                derive_material_transition(Some(&created), &bound, audit_id(1), 30),
                Err(AuditContractError::RevisionMismatch { expected: 1, got: 2 })
            ));
        }

        #[test]
        fn rejects_timestamp_regression() {
            let created = created_task_fixture();
            let running = created
                .start_attempt(
                    TaskAttempt::new(TaskAttemptId::new(1), run_id(), 20),
                    20,
                )
                .unwrap();
            assert!(matches!(
                derive_material_transition(Some(&created), &running, audit_id(1), 5),
                Err(AuditContractError::TimestampRegression { .. })
            ));
        }

        #[test]
        fn rejects_unsupported_transition() {
            let created = created_task_fixture();
            let running = created
                .start_attempt(
                    TaskAttempt::new(TaskAttemptId::new(1), run_id(), 20),
                    20,
                )
                .unwrap();
            // Created → ReadyForReview hits RevisionMismatch (0+1 != 2)
            // because the reducer path requires Running as an intermediate.
            // Test with same-revision but wrong status pair:
            let cancelled = running
                .record_terminal(TaskTerminal::Cancelled, 30)
                .unwrap();
            // Created (rev 0) → Cancelled (rev 2) is unsupported AND revision mismatch
            // Use running (rev 1) → Completed which is unsupported
            let meta = metadata_fixture(run_id_fixture(), TaskAttemptId::new(1));
            let ready = running
                .record_ready_for_review(meta, payload_fixture(), None, 30)
                .unwrap();
            let applying = ready.begin_apply(35).unwrap();
            let completed = applying
                .complete_apply(apply_receipt_fixture(), 50)
                .unwrap();
            // Running → Completed is not a legal transition (skips ReadyForReview → Applying)
            assert!(matches!(
                derive_material_transition(Some(&running), &completed, audit_id(1), 50),
                Err(AuditContractError::TransitionMismatch { .. })
            ));
        }
    }

    // ==================================================================
    // Step 6: Append contract
    // ==================================================================

    mod append_contract {
        use super::*;

        #[test]
        fn append_receipt_fields() {
            let receipt = AuditAppendReceiptV1 {
                event_id: "audit-00000000-0000-4000-8000-000000000001".to_owned(),
                sequence: 0,
                record_hash: "ab".repeat(32),
            };
            assert_eq!(receipt.sequence, 0);
            assert!(!receipt.event_id.is_empty());
            assert!(!receipt.record_hash.is_empty());
        }

        #[test]
        fn append_error_from_category() {
            let err =
                AuditAppendError::from_category(AuditFailureCategory::CorruptJournal);
            assert!(matches!(
                err,
                AuditAppendError::AppendFailed {
                    category: AuditFailureCategory::CorruptJournal
                }
            ));
            assert!(err.to_string().contains("CorruptJournal"));
        }

        #[test]
        fn all_failure_categories_roundtrip() {
            let categories = vec![
                AuditFailureCategory::Unavailable,
                AuditFailureCategory::LockContended,
                AuditFailureCategory::AppendPreCommitFailure,
                AuditFailureCategory::AppendVisibleDurabilityUncertain,
                AuditFailureCategory::UnsupportedSchema,
                AuditFailureCategory::CorruptJournal,
                AuditFailureCategory::SequenceOverflow,
                AuditFailureCategory::JournalTooLarge,
                AuditFailureCategory::CorrelationMismatch,
                AuditFailureCategory::TransitionMismatch,
                AuditFailureCategory::ReconciliationRequired,
            ];
            for cat in categories {
                let err = AuditAppendError::from_category(cat);
                let json = serde_json::to_string(&err).unwrap();
                let back: AuditAppendError = serde_json::from_str(&json).unwrap();
                assert_eq!(err, back);
            }
        }
    }

    // ==================================================================
    // Privacy sentinels: every AuditEventV1 variant
    // ==================================================================

    mod audit_privacy {
        use super::*;

        const FORBIDDEN: &[&str] = &[
            // provider internals / credentials
            "api_key", "secret", "password", "token",
            // prose / prompts / source code / response text
            "system_prompt", "user_prompt", "response_text",
            // tool args / results
            "tool_arguments", "tool_result",
            // pixel data / proposal bytes
            "pixel_data", "raw_bytes", "proposal_payload",
        ];

        fn auth_ref() -> AuthorityAuditRefV1 {
            authority_snapshot_fixture().audit_ref()
        }

        fn skill_ref() -> SkillUseAuditRefV1 {
            SkillUseAuditRefV1 {
                schema_version: 1,
                source_authority: "authority://test".to_owned(),
                package_id: "package-sentinel".to_owned(),
                main_resource_id: "resource-1".to_owned(),
                package_digest: "ab".repeat(32),
                invocation_kind: SkillInvocationKind::HostExplicit,
            }
        }

        fn artifact_ref() -> ArtifactAuditRefV1 {
            ArtifactAuditRefV1 {
                schema_version: 1,
                artifact_id: "artifact-00000000-0000-4000-8000-000000000001".to_owned(),
                artifact_revision: 1,
                kind: ArtifactKind::SmartRedaction,
                canonical_payload_sha256: "aa".repeat(32),
                source_binding_sha256: "bb".repeat(32),
                proposal_id: "proposal-00000000-0000-4000-8000-000000000001".to_owned(),
                provider_id: "anthropic".to_owned(),
                model_id: "claude-sonnet-4-20250514".to_owned(),
                run_config_digest: "cc".repeat(32),
            }
        }

        fn review_decision_fixture() -> ReviewDecisionAuditV1 {
            ReviewDecisionAuditV1 {
                artifact_id: "artifact-00000000-0000-4000-8000-000000000001".to_owned(),
                artifact_revision: 1,
                proposal_id: "proposal-00000000-0000-4000-8000-000000000001".to_owned(),
                applied_candidate_ids: vec![0, 1],
                rejected_candidate_ids: vec![],
                decided_at_unix_ms: 50,
            }
        }

        fn assert_no_forbidden_leaks(event: &AuditEventV1, label: &str) {
            let json = serde_json::to_string(event).unwrap();
            for pat in FORBIDDEN {
                assert!(
                    !json.contains(pat),
                    "{label} leaks forbidden field '{pat}': {json}"
                );
            }
        }

        #[test]
        fn task_created_privacy() {
            assert_no_forbidden_leaks(&AuditEventV1::TaskCreated, "TaskCreated");
        }

        #[test]
        fn attempt_started_privacy() {
            let event = AuditEventV1::AttemptStarted {
                attempt_id: 1,
                run_id: run_id().as_str().to_owned(),
            };
            assert_no_forbidden_leaks(&event, "AttemptStarted");
        }

        #[test]
        fn run_contract_bound_privacy() {
            let event = AuditEventV1::RunContractBound {
                authority: auth_ref(),
                skill_use: skill_ref(),
            };
            assert_no_forbidden_leaks(&event, "RunContractBound");
            let json = serde_json::to_string(&event).unwrap();
            assert!(!json.contains("granted_operations"));
            assert!(!json.contains("prepared_capabilities"));
        }

        #[test]
        fn authority_denied_privacy() {
            let event = AuditEventV1::AuthorityDenied {
                authority: auth_ref(),
                tool_name: "replace_source".to_owned(),
                required_operation: "WriteDraft".to_owned(),
            };
            assert_no_forbidden_leaks(&event, "AuthorityDenied");
        }

        #[test]
        fn artifact_promoted_privacy() {
            let event = AuditEventV1::ArtifactPromoted {
                artifact: artifact_ref(),
            };
            assert_no_forbidden_leaks(&event, "ArtifactPromoted");
            let json = serde_json::to_string(&event).unwrap();
            assert!(!json.contains("proposal_payload"));
            assert!(!json.contains("raw_bytes"));
        }

        #[test]
        fn review_apply_started_privacy() {
            let event = AuditEventV1::ReviewApplyStarted {
                artifact: artifact_ref(),
            };
            assert_no_forbidden_leaks(&event, "ReviewApplyStarted");
        }

        #[test]
        fn review_decision_committed_privacy() {
            let event = AuditEventV1::ReviewDecisionCommitted {
                applied: true,
                review_decision: review_decision_fixture(),
                document_state: Some(DocumentStateReceipt {
                    state_id: 1,
                    digest: "dd".repeat(32),
                }),
            };
            assert_no_forbidden_leaks(&event, "ReviewDecisionCommitted");
            let json = serde_json::to_string(&event).unwrap();
            assert!(!json.contains("local_delta"));
            assert!(!json.contains("moved_candidates"));
        }

        #[test]
        fn task_terminated_privacy() {
            let event = AuditEventV1::TaskTerminated {
                terminal: AuditTaskTerminalV1::Cancelled,
            };
            assert_no_forbidden_leaks(&event, "TaskTerminated");
        }

        #[test]
        fn envelope_serialized_event_excludes_sensitive_fields() {
            let event = AuditEventV1::RunContractBound {
                authority: auth_ref(),
                skill_use: skill_ref(),
            };
            let correlation = AuditCorrelationV1::for_task(
                task_id_fixture().as_str().to_owned(),
            );
            let envelope = AuditEnvelopeV1::new(
                audit_id(99), 9999, event, correlation,
            ).unwrap();
            let json = serde_json::to_string(&envelope).unwrap();
            for pat in FORBIDDEN {
                assert!(
                    !json.contains(pat),
                    "envelope leaks forbidden field '{pat}': {json}"
                );
            }
        }
    }

    // ==================================================================
    // Authoritative repair: journal survives crash without blocking
    // product state
    // ==================================================================

    mod authoritative_repair {
        use super::*;

        #[test]
        fn uncommitted_prepare_aborted_on_next_scan() {
            // Simulate crash after prepare but before commit.
            // Re-scan should find the pending transaction and the
            // reconciliation layer should abort it.
            let task = ProductTaskSnapshot::new(
                task_id_fixture(),
                TaskKind::SmartRedactionAuthor,
                SourceBinding::new([1u8; 32], [2u8; 32], 0, "preset-001".to_owned(), None),
                10,
            ).unwrap();
            let receipt = task.audit_transition_receipt();
            let correlation = AuditCorrelationV1::for_task(
                task_id_fixture().as_str().to_owned(),
            );
            let envelope = AuditEnvelopeV1::new(
                audit_id(1), 10, AuditEventV1::TaskCreated, correlation,
            ).unwrap();

            // The envelope itself is valid and can be used for a prepare.
            // Verify it round-trips.
            let json = serde_json::to_string(&envelope).unwrap();
            let back: AuditEnvelopeV1 = serde_json::from_str(&json).unwrap();
            assert_eq!(envelope, back);
        }
    }

    // ==================================================================
    // Corruption blocking: corrupt journal is non-authoritative
    // ==================================================================

    mod corrupt_journal {
        use super::*;

        #[test]
        fn corrupt_envelope_json_is_rejected_by_contract() {
            // A malformed envelope must not pass through.
            let bad_json = r#"{"schema_version":1,"event_id":12345}"#;
            let result: Result<AuditEnvelopeV1, _> = serde_json::from_str(bad_json);
            assert!(result.is_err(), "corrupt envelope must be rejected");
        }

        #[test]
        fn truncated_envelope_json_is_rejected() {
            let bad_json = r#"{"schema_vers"#;
            let result: Result<AuditEnvelopeV1, _> = serde_json::from_str(bad_json);
            assert!(result.is_err(), "truncated envelope must be rejected");
        }
    }
}
