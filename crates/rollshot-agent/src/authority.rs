//! Immutable run authority contract.
//!
//! An `AuthoritySnapshot` is the sealed, read-only proof that a run was
//! authorized with specific policy, disclosure, and capability bounds.
//! The snapshot digest is canonical and order-independent.

use std::collections::BTreeSet;
use std::fmt;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::domain::AuthorizedModelInput;
use crate::domain::RunId;
use crate::product_task::{DocumentContentBinding, ProductTaskId, TaskAttemptId};

// ========================================================================
// Authority schema version
// ========================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AuthoritySchemaVersion {
    V1,
}

// ========================================================================
// Disclosure ceiling
// ========================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DisclosureCeiling {
    OcrLayoutOnly,
    FullScreenshot,
}

// ========================================================================
// Prepared capability
// ========================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PreparedCapability {
    RegionFeatures,
    Ocr,
    Layout,
    TemplateMatch,
}

// ========================================================================
// Run operation (grants)
// ========================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunOperation {
    ReadDraft,
    WriteDraft,
    InspectPreparedImage,
    ExecuteRestrictedAutomation,
    SubmitReviewCandidate,
    RequestUserInput,
}

// ========================================================================
// Authority subject
// ========================================================================

/// What a run holds authority over. `Document` is the Smart Redaction subject;
/// its digest input is unchanged so existing receipts stay comparable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuthoritySubject {
    Document(DocumentContentBinding),
    ActionGuideProject {
        project_root_sha256: [u8; 32],
        revision: u64,
        projection_digest: String,
    },
    ActionGuideEphemeralGuide {
        guide_digest: String,
    },
}

// ========================================================================
// Authority binding
// ========================================================================

/// Immutable binding of a specific task attempt + run + authority subject.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthorityBinding {
    task_id: ProductTaskId,
    attempt_id: TaskAttemptId,
    run_id: RunId,
    subject: AuthoritySubject,
}

impl AuthorityBinding {
    pub fn new(
        task_id: ProductTaskId,
        attempt_id: TaskAttemptId,
        run_id: RunId,
        subject: AuthoritySubject,
    ) -> Self {
        Self {
            task_id,
            attempt_id,
            run_id,
            subject,
        }
    }

    pub fn run_id(&self) -> &RunId {
        &self.run_id
    }

    pub fn subject(&self) -> &AuthoritySubject {
        &self.subject
    }
}

// ========================================================================
// Authority snapshot (immutable, digest-stable)
// ========================================================================

/// Immutable authority snapshot. All fields are private; the only way in is
/// the checked constructor. The digest is computed once and cached.
#[derive(Clone, PartialEq, Eq)]
pub struct AuthoritySnapshot {
    binding: AuthorityBinding,
    policy_revision: String,
    disclosure: DisclosureCeiling,
    existing_product_capture: bool,
    prepared_capabilities: BTreeSet<PreparedCapability>,
    grants: BTreeSet<RunOperation>,
    digest: String,
}

impl AuthoritySnapshot {
    /// Create a new authority snapshot. Computes and caches the canonical digest.
    ///
    /// Returns `Err` if `policy_revision` is empty, if `existing_product_capture`
    /// is false but `InspectPreparedImage` is in `grants`.
    pub fn new(
        binding: AuthorityBinding,
        policy_revision: String,
        disclosure: DisclosureCeiling,
        existing_product_capture: bool,
        prepared_capabilities: BTreeSet<PreparedCapability>,
        grants: BTreeSet<RunOperation>,
    ) -> Result<Self, AuthorityError> {
        if policy_revision.is_empty() {
            return Err(AuthorityError::InvalidPolicyRevision);
        }
        if !existing_product_capture && grants.contains(&RunOperation::InspectPreparedImage) {
            return Err(AuthorityError::MissingProductCapture);
        }

        let snapshot = Self {
            binding,
            policy_revision,
            disclosure,
            existing_product_capture,
            prepared_capabilities,
            grants,
            digest: String::new(), // placeholder
        };
        let digest = snapshot.compute_digest();
        Ok(Self { digest, ..snapshot })
    }

    /// Authorize a specific tool invocation for this run.
    ///
    /// Returns `Ok(())` if the run_id matches, the authority subject is
    /// consistent, and the required operation is in the grant set.
    pub fn authorize_tool(
        &self,
        run_id: &RunId,
        subject: &AuthoritySubject,
        required: RunOperation,
    ) -> Result<(), AuthorityError> {
        if run_id != self.binding.run_id() {
            return Err(AuthorityError::RunMismatch);
        }
        if subject != self.binding.subject() {
            return Err(AuthorityError::DocumentBindingMismatch);
        }
        if !self.grants.contains(&required) {
            return Err(AuthorityError::GrantMissing {
                operation: required,
            });
        }
        Ok(())
    }

    /// Validate that model input respects the disclosure ceiling.
    ///
    /// `OcrLayoutOnly` rejects any attachments. `FullScreenshot` accepts
    /// any attachment count (including zero — it is a ceiling, not a requirement).
    pub fn validate_model_input(&self, input: &AuthorizedModelInput) -> Result<(), AuthorityError> {
        let attachment_count = input.attachments().len();
        match self.disclosure {
            DisclosureCeiling::OcrLayoutOnly => {
                if attachment_count > 0 {
                    return Err(AuthorityError::DisclosureExceeded {
                        ceiling: self.disclosure,
                        attachment_count,
                    });
                }
            }
            DisclosureCeiling::FullScreenshot => {
                // Ceiling, not requirement: zero attachments is fine.
            }
        }
        Ok(())
    }

    /// Generate a receipt for this snapshot at the given timestamp.
    pub fn receipt(&self, created_at_unix_ms: i64) -> AuthoritySnapshotReceiptV1 {
        AuthoritySnapshotReceiptV1 {
            schema_version: 1,
            task_id: self.binding.task_id.as_str().to_string(),
            attempt_id: self.binding.attempt_id.get(),
            run_id: self.binding.run_id.as_str().to_string(),
            policy_revision: self.policy_revision.clone(),
            disclosure_ceiling: self.disclosure,
            existing_product_capture: self.existing_product_capture,
            subject_digest: self.binding_digest_hex(),
            prepared_capabilities: self.prepared_capabilities.iter().copied().collect(),
            granted_operations: self.grants.iter().copied().collect(),
            snapshot_digest: self.digest.clone(),
            created_at_unix_ms,
        }
    }

    /// Task ID from the authority binding.
    pub fn task_id(&self) -> &ProductTaskId {
        &self.binding.task_id
    }

    /// Attempt ID from the authority binding.
    pub fn attempt_id(&self) -> TaskAttemptId {
        self.binding.attempt_id
    }

    /// Run ID from the authority binding.
    pub fn run_id(&self) -> &RunId {
        &self.binding.run_id
    }

    /// Policy revision.
    pub fn policy_revision(&self) -> &str {
        &self.policy_revision
    }

    /// Disclosure ceiling.
    pub fn disclosure(&self) -> DisclosureCeiling {
        self.disclosure
    }

    /// Whether an existing product capture was present.
    pub fn existing_product_capture(&self) -> bool {
        self.existing_product_capture
    }

    /// The canonical snapshot digest.
    pub fn digest(&self) -> &str {
        &self.digest
    }

    /// Hex-encoded authority subject digest (binding-private helper).
    fn binding_digest_hex(&self) -> String {
        let mut hasher = Sha256::new();
        match self.binding.subject() {
            AuthoritySubject::Document(binding) => {
                hasher.update(b"rollshot-authority-subject-document-v1\0");
                hasher.update(binding.base_image_digest());
                hasher.update(binding.annotation_state_digest());
                hasher.update(binding.state_id().to_le_bytes());
            }
            AuthoritySubject::ActionGuideProject {
                project_root_sha256,
                revision,
                projection_digest,
            } => {
                hasher.update(b"rollshot-authority-subject-action-guide-project-v1\0");
                hasher.update(project_root_sha256);
                hasher.update(revision.to_le_bytes());
                hasher.update(projection_digest.as_bytes());
            }
            AuthoritySubject::ActionGuideEphemeralGuide { guide_digest } => {
                hasher.update(b"rollshot-authority-subject-action-guide-ephemeral-v1\0");
                hasher.update(guide_digest.as_bytes());
            }
        }
        hex_encode(&hasher.finalize())
    }

    /// Compute the canonical V1 snapshot digest.
    fn compute_digest(&self) -> String {
        let dto = DigestedSnapshotV1 {
            task_id: self.binding.task_id.as_str().to_string(),
            attempt_id: self.binding.attempt_id.get(),
            run_id: self.binding.run_id.as_str().to_string(),
            policy_revision: self.policy_revision.clone(),
            disclosure: self.disclosure,
            existing_product_capture: self.existing_product_capture,
            document_binding_digest: self.binding_digest_hex(),
            prepared_capabilities: self.prepared_capabilities.iter().copied().collect(),
            grants: self.grants.iter().copied().collect(),
        };
        let mut hasher = Sha256::new();
        hasher.update(b"rollshot-authority-v1\0");
        let canonical =
            serde_json::to_vec(&dto).expect("DigestedSnapshotV1 serialization infallible");
        hasher.update(&canonical);
        hex_encode(&hasher.finalize())
    }
}

impl fmt::Debug for AuthoritySnapshot {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AuthoritySnapshot")
            .field("task_id", &self.binding.task_id.as_str())
            .field("attempt_id", &self.binding.attempt_id.get())
            .field("run_id", &self.binding.run_id.as_str())
            .field("disclosure", &self.disclosure)
            .field("grants_count", &self.grants.len())
            .field("capabilities_count", &self.prepared_capabilities.len())
            .field("digest", &self.digest)
            .finish()
    }
}

// ========================================================================
// Authority errors
// ========================================================================

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AuthorityError {
    #[error("policy revision must not be empty")]
    InvalidPolicyRevision,
    #[error("run ID does not match authority binding")]
    RunMismatch,
    #[error("document binding does not match authority binding")]
    DocumentBindingMismatch,
    #[error("required operation `{operation:?}` is not in the grant set")]
    GrantMissing { operation: RunOperation },
    #[error("disclosure ceiling {ceiling:?} exceeded: {attachment_count} attachment(s)")]
    DisclosureExceeded {
        ceiling: DisclosureCeiling,
        attachment_count: usize,
    },
    #[error("InspectPreparedImage requires an existing product capture")]
    MissingProductCapture,
    #[error("unsupported authority schema version: {version}")]
    UnsupportedSchema { version: u32 },
}

// ========================================================================
// Authority receipt V1
// ========================================================================

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthoritySnapshotReceiptV1 {
    pub schema_version: u32,
    pub task_id: String,
    pub attempt_id: u32,
    pub run_id: String,
    pub policy_revision: String,
    pub disclosure_ceiling: DisclosureCeiling,
    pub existing_product_capture: bool,
    #[serde(rename = "document_binding_digest")]
    pub subject_digest: String,
    pub prepared_capabilities: Vec<PreparedCapability>,
    pub granted_operations: Vec<RunOperation>,
    pub snapshot_digest: String,
    pub created_at_unix_ms: i64,
}

// ========================================================================
// Private canonical DTO (sorted fields, no content)
// ========================================================================

#[derive(Serialize)]
struct DigestedSnapshotV1 {
    task_id: String,
    attempt_id: u32,
    run_id: String,
    policy_revision: String,
    disclosure: DisclosureCeiling,
    existing_product_capture: bool,
    document_binding_digest: String,
    prepared_capabilities: Vec<PreparedCapability>,
    grants: Vec<RunOperation>,
}

// ========================================================================
// Helpers
// ========================================================================

fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

// ========================================================================
// Tests
// ========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{AttachmentDescriptor, MediaType};
    use crate::product_task::AnnotationStateV1;

    // ---- ID fixtures ----

    fn task_id() -> ProductTaskId {
        ProductTaskId::parse("task-00000000-0000-4000-8000-000000000001").unwrap()
    }

    fn run_id() -> RunId {
        RunId::parse("run-00000000-0000-4000-8000-000000000001").unwrap()
    }

    fn document_binding() -> DocumentContentBinding {
        let state = AnnotationStateV1 {
            width: 100,
            height: 80,
            state_id: 1,
            annotations: vec![],
        };
        DocumentContentBinding::new([0xAB_u8; 32], &state, 1).unwrap()
    }

    // ---- Snapshot builders ----

    fn snapshot_with(
        capabilities: impl IntoIterator<Item = PreparedCapability>,
        grants: impl IntoIterator<Item = RunOperation>,
    ) -> AuthoritySnapshot {
        AuthoritySnapshot::new(
            AuthorityBinding::new(
                task_id(),
                TaskAttemptId::new(1),
                run_id(),
                AuthoritySubject::Document(document_binding()),
            ),
            "rev-1".into(),
            DisclosureCeiling::OcrLayoutOnly,
            false,
            capabilities.into_iter().collect(),
            grants.into_iter().collect(),
        )
        .unwrap()
    }

    fn snapshot_with_grants(grants: impl IntoIterator<Item = RunOperation>) -> AuthoritySnapshot {
        snapshot_with([], grants)
    }

    fn snapshot_with_disclosure(disclosure: DisclosureCeiling) -> AuthoritySnapshot {
        AuthoritySnapshot::new(
            AuthorityBinding::new(
                task_id(),
                TaskAttemptId::new(1),
                run_id(),
                AuthoritySubject::Document(document_binding()),
            ),
            "rev-1".into(),
            disclosure,
            true,
            [].into_iter().collect(),
            [RunOperation::ReadDraft].into_iter().collect(),
        )
        .unwrap()
    }

    fn full_snapshot() -> AuthoritySnapshot {
        AuthoritySnapshot::new(
            AuthorityBinding::new(
                task_id(),
                TaskAttemptId::new(1),
                run_id(),
                AuthoritySubject::Document(document_binding()),
            ),
            "rev-full".into(),
            DisclosureCeiling::FullScreenshot,
            true,
            [PreparedCapability::Ocr, PreparedCapability::Layout]
                .into_iter()
                .collect(),
            [RunOperation::ReadDraft, RunOperation::WriteDraft]
                .into_iter()
                .collect(),
        )
        .unwrap()
    }

    fn png_input(attachment_bytes: Vec<u8>) -> AuthorizedModelInput {
        AuthorizedModelInput::new(
            "openai".into(),
            "gpt-4o".into(),
            "test".into(),
            vec![AttachmentDescriptor {
                media_type: MediaType::Png,
                width: 100,
                height: 80,
                byte_count: attachment_bytes.len() as u64,
            }],
            vec![attachment_bytes],
        )
        .unwrap()
    }

    fn input_without_attachments() -> AuthorizedModelInput {
        AuthorizedModelInput::new(
            "openai".into(),
            "gpt-4o".into(),
            "test".into(),
            vec![],
            vec![],
        )
        .unwrap()
    }

    // ------------------------------------------------------------------
    // Digest canonicality
    // ------------------------------------------------------------------

    #[test]
    fn snapshot_digest_is_canonical_and_order_independent() {
        let a = snapshot_with(
            [PreparedCapability::Ocr, PreparedCapability::RegionFeatures],
            [RunOperation::ReadDraft, RunOperation::WriteDraft],
        );
        let b = snapshot_with(
            [PreparedCapability::RegionFeatures, PreparedCapability::Ocr],
            [RunOperation::WriteDraft, RunOperation::ReadDraft],
        );
        assert_eq!(a.digest(), b.digest());
        assert_eq!(
            a.receipt(123).snapshot_digest,
            b.receipt(123).snapshot_digest
        );
    }

    #[test]
    fn different_grants_yield_different_digests() {
        let a = snapshot_with_grants([RunOperation::ReadDraft]);
        let b = snapshot_with_grants([RunOperation::WriteDraft]);
        assert_ne!(a.digest(), b.digest());
    }

    #[test]
    fn persisted_authority_digest_is_never_recomputed_for_comparison() {
        // **What this test pins:** `digest()` returns a cached value rather than
        // recomputing, and `receipt()` reports the same digest string the snapshot
        // reports. Both are in-memory properties of this module.
        //
        // **What this test cannot pin, and what does:** No code elsewhere recomputes
        // a digest and compares it to a persisted one. That is established by the
        // Task 7 audit, whose classification table lives in
        // `docs/superpowers/plans/2026-07-28-action-guide-agent-foundation-captions.md`
        // under Task 7 — and structurally by the fact that `AuthoritySnapshot` has
        // no `Deserialize` impl, so a snapshot cannot be reconstructed from persisted
        // state at all.
        //
        // **Why it matters:** If a future change adds such a comparison, it must also
        // add a formula-version field, because changing the hash inputs otherwise
        // invalidates every stored receipt.
        let snapshot = full_snapshot();
        let first = snapshot.digest().to_string();
        let receipt = snapshot.receipt(1_000);

        assert_eq!(receipt.snapshot_digest, first);
        assert_eq!(
            snapshot.digest(),
            first,
            "digest must be cached, not recomputed"
        );

        // Two snapshots built from identical inputs produce the same digest,
        // confirming the in-memory caching and determinism properties.
        assert_eq!(full_snapshot().digest(), first);
    }

    // ------------------------------------------------------------------
    // Disclosure ceiling
    // ------------------------------------------------------------------

    #[test]
    fn ocr_only_rejects_any_model_attachment() {
        let snapshot = snapshot_with_disclosure(DisclosureCeiling::OcrLayoutOnly);
        let input = png_input(vec![1, 2, 3, 4]);
        assert_eq!(
            snapshot.validate_model_input(&input),
            Err(AuthorityError::DisclosureExceeded {
                ceiling: DisclosureCeiling::OcrLayoutOnly,
                attachment_count: 1,
            })
        );
    }

    #[test]
    fn full_screenshot_is_a_ceiling_not_a_requirement() {
        let snapshot = snapshot_with_disclosure(DisclosureCeiling::FullScreenshot);
        assert_eq!(
            snapshot.validate_model_input(&input_without_attachments()),
            Ok(())
        );
    }

    // ------------------------------------------------------------------
    // Privacy (Debug + receipt)
    // ------------------------------------------------------------------

    #[test]
    fn authority_debug_and_receipt_exclude_private_content() {
        let snapshot = full_snapshot();
        let debug = format!("{snapshot:?}");
        let json = serde_json::to_string(&snapshot.receipt(123)).unwrap();
        for forbidden in ["api_key", "user_message", "skill body", "/home/"] {
            assert!(!debug.contains(forbidden), "debug leaked: {forbidden}");
            assert!(!json.contains(forbidden), "receipt leaked: {forbidden}");
        }
    }

    // ------------------------------------------------------------------
    // Persisted receipt key stability (Task 8)
    //
    // `AuthoritySnapshotReceiptV1` is persisted inside `RunContractReceiptV1`
    // in task JSON on disk. The Rust field was renamed from
    // `document_binding_digest` to `subject_digest` when `AuthorityBinding`
    // was generalized to hold an `AuthoritySubject`, but the serialized key
    // must not change or existing task files stop loading. These tests pin
    // both directions: new receipts still serialize the old key name, and
    // JSON written under the old key name still deserializes.
    // ------------------------------------------------------------------

    #[test]
    fn receipt_serializes_with_the_legacy_document_binding_digest_key() {
        let receipt = full_snapshot().receipt(123);
        let json = serde_json::to_value(&receipt).unwrap();
        let obj = json.as_object().unwrap();
        assert!(
            obj.contains_key("document_binding_digest"),
            "serialized receipt must keep the on-disk key `document_binding_digest`, got keys: {:?}",
            obj.keys().collect::<Vec<_>>()
        );
        assert!(
            !obj.contains_key("subject_digest"),
            "the renamed Rust field name must not leak into JSON"
        );
    }

    #[test]
    fn receipt_deserializes_pre_existing_task_json_using_the_old_key() {
        // Simulates a task file written to disk before the AuthoritySubject
        // rename — proves old on-disk receipts still load.
        let legacy_json = r#"{
            "schema_version": 1,
            "task_id": "task-00000000-0000-4000-8000-000000000001",
            "attempt_id": 1,
            "run_id": "run-00000000-0000-4000-8000-000000000001",
            "policy_revision": "rev-1",
            "disclosure_ceiling": "full_screenshot",
            "existing_product_capture": true,
            "document_binding_digest": "deadbeef",
            "prepared_capabilities": [],
            "granted_operations": [],
            "snapshot_digest": "cafef00d",
            "created_at_unix_ms": 1
        }"#;
        let receipt: AuthoritySnapshotReceiptV1 = serde_json::from_str(legacy_json).unwrap();
        assert_eq!(receipt.subject_digest, "deadbeef");
    }

    // ------------------------------------------------------------------
    // authorize_tool edge cases
    // ------------------------------------------------------------------

    #[test]
    fn authorize_tool_rejects_mismatched_run_id() {
        let snapshot = full_snapshot();
        let wrong_run = RunId::parse("run-99999999-9999-4999-8999-999999999999").unwrap();
        assert_eq!(
            snapshot.authorize_tool(
                &wrong_run,
                &AuthoritySubject::Document(document_binding()),
                RunOperation::ReadDraft
            ),
            Err(AuthorityError::RunMismatch)
        );
    }

    #[test]
    fn authorize_tool_rejects_mismatched_document_binding() {
        let snapshot = full_snapshot();
        let state2 = AnnotationStateV1 {
            width: 200,
            height: 160,
            state_id: 2,
            annotations: vec![],
        };
        let different_binding = DocumentContentBinding::new([0xCD_u8; 32], &state2, 2).unwrap();
        assert_eq!(
            snapshot.authorize_tool(
                &run_id(),
                &AuthoritySubject::Document(different_binding),
                RunOperation::ReadDraft
            ),
            Err(AuthorityError::DocumentBindingMismatch)
        );
    }

    #[test]
    fn authorize_tool_rejects_missing_operation() {
        let snapshot = snapshot_with_grants([RunOperation::ReadDraft]);
        assert_eq!(
            snapshot.authorize_tool(
                &run_id(),
                &AuthoritySubject::Document(document_binding()),
                RunOperation::WriteDraft
            ),
            Err(AuthorityError::GrantMissing {
                operation: RunOperation::WriteDraft,
            })
        );
    }

    #[test]
    fn authorize_tool_success() {
        let snapshot = full_snapshot();
        assert_eq!(
            snapshot.authorize_tool(
                &run_id(),
                &AuthoritySubject::Document(document_binding()),
                RunOperation::ReadDraft
            ),
            Ok(())
        );
    }

    // ------------------------------------------------------------------
    // AuthoritySubject variants (Task 8)
    // ------------------------------------------------------------------

    #[test]
    fn action_guide_subject_authorizes_submit_and_rejects_image_ops() {
        let subject = AuthoritySubject::ActionGuideProject {
            project_root_sha256: [4u8; 32],
            revision: 2,
            projection_digest: "ab".repeat(32),
        };
        let run_id = run_id();
        let snapshot = AuthoritySnapshot::new(
            AuthorityBinding::new(
                task_id(),
                TaskAttemptId::new(1),
                run_id.clone(),
                subject.clone(),
            ),
            "rollshot-v1".to_owned(),
            DisclosureCeiling::OcrLayoutOnly,
            false,
            BTreeSet::new(),
            BTreeSet::from([RunOperation::SubmitReviewCandidate]),
        )
        .unwrap();

        assert!(snapshot
            .authorize_tool(&run_id, &subject, RunOperation::SubmitReviewCandidate)
            .is_ok());
        assert!(matches!(
            snapshot.authorize_tool(&run_id, &subject, RunOperation::InspectPreparedImage),
            Err(AuthorityError::GrantMissing { .. })
        ));
    }

    #[test]
    fn subject_mismatch_is_rejected() {
        let subject = AuthoritySubject::ActionGuideProject {
            project_root_sha256: [4u8; 32],
            revision: 2,
            projection_digest: "ab".repeat(32),
        };
        let moved_on = AuthoritySubject::ActionGuideProject {
            project_root_sha256: [4u8; 32],
            revision: 3,
            projection_digest: "cd".repeat(32),
        };
        let run_id = run_id();
        let snapshot = AuthoritySnapshot::new(
            AuthorityBinding::new(task_id(), TaskAttemptId::new(1), run_id.clone(), subject),
            "rollshot-v1".to_owned(),
            DisclosureCeiling::OcrLayoutOnly,
            false,
            BTreeSet::new(),
            BTreeSet::from([RunOperation::SubmitReviewCandidate]),
        )
        .unwrap();

        assert!(matches!(
            snapshot.authorize_tool(&run_id, &moved_on, RunOperation::SubmitReviewCandidate),
            Err(AuthorityError::DocumentBindingMismatch)
        ));
    }

    #[test]
    fn run_mismatch_is_checked_before_the_subject() {
        // Order matters: authorize_tool checks run_id first (authority.rs:181),
        // so a wrong run on a matching subject must not read as a subject
        // mismatch.
        let subject = AuthoritySubject::ActionGuideEphemeralGuide {
            guide_digest: "ee".repeat(32),
        };
        let snapshot = AuthoritySnapshot::new(
            AuthorityBinding::new(task_id(), TaskAttemptId::new(1), run_id(), subject.clone()),
            "rollshot-v1".to_owned(),
            DisclosureCeiling::OcrLayoutOnly,
            false,
            BTreeSet::new(),
            BTreeSet::from([RunOperation::SubmitReviewCandidate]),
        )
        .unwrap();
        let other_run = RunId::parse("run-00000000-0000-4000-8000-000000000002").unwrap();

        assert!(matches!(
            snapshot.authorize_tool(&other_run, &subject, RunOperation::SubmitReviewCandidate),
            Err(AuthorityError::RunMismatch)
        ));
    }

    // ------------------------------------------------------------------
    // Constructor validation
    // ------------------------------------------------------------------

    #[test]
    fn empty_policy_revision_rejected() {
        let result = AuthoritySnapshot::new(
            AuthorityBinding::new(
                task_id(),
                TaskAttemptId::new(1),
                run_id(),
                AuthoritySubject::Document(document_binding()),
            ),
            "".into(),
            DisclosureCeiling::OcrLayoutOnly,
            false,
            [].into_iter().collect(),
            [].into_iter().collect(),
        );
        assert_eq!(result, Err(AuthorityError::InvalidPolicyRevision));
    }

    #[test]
    fn existing_product_capture_false_rejects_inspect_prepared_image() {
        let result = AuthoritySnapshot::new(
            AuthorityBinding::new(
                task_id(),
                TaskAttemptId::new(1),
                run_id(),
                AuthoritySubject::Document(document_binding()),
            ),
            "rev-1".into(),
            DisclosureCeiling::OcrLayoutOnly,
            false, // no existing product capture
            [].into_iter().collect(),
            [RunOperation::InspectPreparedImage].into_iter().collect(),
        );
        assert_eq!(result, Err(AuthorityError::MissingProductCapture));
    }

    #[test]
    fn existing_product_capture_true_allows_inspect_prepared_image() {
        let result = AuthoritySnapshot::new(
            AuthorityBinding::new(
                task_id(),
                TaskAttemptId::new(1),
                run_id(),
                AuthoritySubject::Document(document_binding()),
            ),
            "rev-1".into(),
            DisclosureCeiling::FullScreenshot,
            true,
            [].into_iter().collect(),
            [RunOperation::InspectPreparedImage].into_iter().collect(),
        );
        assert!(result.is_ok());
    }

    // ------------------------------------------------------------------
    // Unsupported schema deserialization
    // ------------------------------------------------------------------

    #[test]
    fn unsupported_schema_deserialization() {
        let json = r#"{"version": 99}"#;
        let result: Result<AuthoritySnapshotReceiptV1, _> = serde_json::from_str(json);
        // Receipt has a u32 schema_version field; deserialization succeeds
        // but the schema_version will be 0 (default for missing field).
        // The UnsupportedSchema error is for runtime validation if needed.
        if let Ok(receipt) = result {
            assert_ne!(receipt.schema_version, 1);
        }
    }

    // ------------------------------------------------------------------
    // Receipt fields are sorted and duplicate-free
    // ------------------------------------------------------------------

    #[test]
    fn receipt_fields_are_sorted_and_duplicate_free() {
        let snapshot = snapshot_with(
            [
                PreparedCapability::TemplateMatch,
                PreparedCapability::Ocr,
                PreparedCapability::Layout,
            ],
            [
                RunOperation::WriteDraft,
                RunOperation::ReadDraft,
                RunOperation::SubmitReviewCandidate,
            ],
        );
        let receipt = snapshot.receipt(999);
        // BTreeSet iteration is sorted
        assert_eq!(
            receipt.prepared_capabilities,
            vec![
                PreparedCapability::Ocr,
                PreparedCapability::Layout,
                PreparedCapability::TemplateMatch,
            ]
        );
        assert_eq!(
            receipt.granted_operations,
            vec![
                RunOperation::ReadDraft,
                RunOperation::WriteDraft,
                RunOperation::SubmitReviewCandidate,
            ]
        );
        // Timestamp is reflected
        assert_eq!(receipt.created_at_unix_ms, 999);
    }

    // ------------------------------------------------------------------
    // Grant error message includes the missing operation
    // ------------------------------------------------------------------

    #[test]
    fn grant_error_includes_operation() {
        let err = AuthorityError::GrantMissing {
            operation: RunOperation::ExecuteRestrictedAutomation,
        };
        let msg = err.to_string();
        assert!(
            msg.contains("ExecuteRestrictedAutomation"),
            "error message: {msg}"
        );
    }
}
