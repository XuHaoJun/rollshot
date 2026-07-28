//! Pure unresolved-transaction classification from verified journal state
//! plus authoritative task presence/revision/transition receipt.
//!
//! `classify_unresolved` compares a prepared transaction's expected and
//! replacement revisions and privacy-safe replacement receipt against the
//! authoritative task snapshot. It decides whether the transaction should
//! be committed, aborted, or rejected as requiring manual reconciliation.

use rollshot_agent::audit::AuditTaskStateReceiptV1;

use super::record::{AuditAbortCategory, PreparedTransactionV1};
use super::AuditStoreError;

// ============================================================================
// Reconcile decision
// ============================================================================

/// Decision for an unresolved prepared transaction.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum ReconcileDecision {
    /// The exact replacement revision is now authoritative: commit the
    /// audit record. Carries the replacement receipt for verification.
    Commit,
    /// The expected revision is still authoritative: the task mutation
    /// never landed. Abort the audit record.
    Abort(AuditAbortCategory),
}

// ============================================================================
// Pure classification
// ============================================================================

/// Classify an unresolved prepared transaction against the authoritative
/// task state.
///
/// `authoritative_receipt` is `Some` when the task file exists on disk,
/// `None` when the task file is absent (the task was never created or was
/// pruned).
///
/// # Decision matrix
///
/// | Authoritative receipt     | Condition                       | Decision  |
/// |--------------------------|---------------------------------|-----------|
/// | Some(receipt)             | receipt.revision == replacement  | Commit    |
/// | Some(receipt)             | receipt.revision == expected     | Abort     |
/// | Some(receipt)             | revision is unrelated            | Reject    |
/// | None                      | always                           | Abort     |
pub(crate) fn classify_unresolved(
    prepared: &PreparedTransactionV1,
    authoritative_receipt: Option<&AuditTaskStateReceiptV1>,
) -> Result<ReconcileDecision, AuditStoreError> {
    let task_id = prepared.envelope.correlation().task_id();

    match authoritative_receipt {
        Some(receipt) => {
            // Validate task ID matches.
            if receipt.task_id != task_id {
                return Err(AuditStoreError::ReconciliationRequired {
                    task_id: task_id.to_owned(),
                    reason: format!(
                        "task ID mismatch: prepared={}, authoritative={}",
                        task_id, receipt.task_id
                    ),
                });
            }

            if receipt.snapshot_revision == prepared.replacement_revision {
                // The exact replacement is now authoritative: the task
                // mutation was committed. The prepared transition receipt
                // must match it field for field — a matching revision alone
                // does not prove this transaction produced the state on
                // disk (spec §9.4).
                if *receipt != prepared.replacement_receipt {
                    return Err(AuditStoreError::ReconciliationRequired {
                        task_id: task_id.to_owned(),
                        reason: format!(
                            "transition receipt mismatch at revision {}",
                            receipt.snapshot_revision
                        ),
                    });
                }
                Ok(ReconcileDecision::Commit)
            } else if receipt.snapshot_revision == prepared.expected_revision {
                // The expected revision is still authoritative: the task
                // mutation never landed. Abort the audit record.
                Ok(ReconcileDecision::Abort(
                    AuditAbortCategory::StateNotCommitted,
                ))
            } else {
                // Unrelated revision: neither expected nor replacement.
                // This indicates intervening mutations that make the
                // transaction outcome unknowable.
                Err(AuditStoreError::ReconciliationRequired {
                    task_id: task_id.to_owned(),
                    reason: format!(
                        "unrelated revision: expected={}, replacement={}, authoritative={}",
                        prepared.expected_revision,
                        prepared.replacement_revision,
                        receipt.snapshot_revision,
                    ),
                })
            }
        }
        None => {
            // Task file absent: the task was never created or was pruned.
            // Abort the audit record.
            Ok(ReconcileDecision::Abort(
                AuditAbortCategory::StateNotCommitted,
            ))
        }
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use rollshot_agent::audit::{
        AuditCorrelationV1, AuditEnvelopeV1, AuditEventId, AuditEventV1, AuditTaskStateReceiptV1,
        AuditTaskStatusV1,
    };
    use rollshot_agent::product_task::ProductTaskId;

    use crate::result_workspace::workbench::audit_store::record::{
        AuditAbortCategory, AuditTransactionId, PreparedTransactionV1,
    };

    // ------------------------------------------------------------------
    // Fixtures
    // ------------------------------------------------------------------

    fn task_id() -> ProductTaskId {
        ProductTaskId::parse("task-00000000-0000-4000-8000-000000000001").unwrap()
    }

    fn audit_event_id(n: u64) -> AuditEventId {
        AuditEventId::parse(format!("audit-00000000-0000-4000-8000-{n:012x}")).unwrap()
    }

    fn task_receipt(revision: u32) -> AuditTaskStateReceiptV1 {
        AuditTaskStateReceiptV1 {
            task_id: task_id().as_str().to_owned(),
            status: AuditTaskStatusV1::Running,
            snapshot_revision: revision,
            created_at_unix_ms: 10,
            updated_at_unix_ms: 20,
            artifact: None,
            review_decision: None,
        }
    }

    fn test_envelope() -> AuditEnvelopeV1 {
        AuditEnvelopeV1::new(
            audit_event_id(1),
            20,
            AuditEventV1::TaskCreated,
            AuditCorrelationV1::for_task(task_id().as_str().to_owned()),
        )
        .unwrap()
    }

    fn prepared_fixture(expected: u32, replacement: u32) -> PreparedTransactionV1 {
        PreparedTransactionV1 {
            transaction_id: AuditTransactionId::new("txn-00000000-0000-4000-8000-000000000001"),
            event_id: audit_event_id(1),
            envelope: test_envelope(),
            expected_revision: expected,
            replacement_revision: replacement,
            replacement_receipt: AuditTaskStateReceiptV1 {
                task_id: task_id().as_str().to_owned(),
                status: AuditTaskStatusV1::Running,
                snapshot_revision: replacement,
                created_at_unix_ms: 10,
                updated_at_unix_ms: 20,
                artifact: None,
                review_decision: None,
            },
        }
    }

    // ==================================================================
    // Transition (update/CAS) decision matrix
    // ==================================================================

    #[test]
    fn unresolved_prepare_commits_when_exact_replacement_is_authoritative() {
        let prepared = prepared_fixture(4, 5);
        assert_eq!(
            classify_unresolved(&prepared, Some(&task_receipt(5))).unwrap(),
            ReconcileDecision::Commit
        );
    }

    #[test]
    fn unresolved_prepare_aborts_when_expected_revision_remains_authoritative() {
        let prepared = prepared_fixture(4, 5);
        assert_eq!(
            classify_unresolved(&prepared, Some(&task_receipt(4))).unwrap(),
            ReconcileDecision::Abort(AuditAbortCategory::StateNotCommitted)
        );
    }

    #[test]
    fn unresolved_prepare_rejects_unrelated_revision() {
        let prepared = prepared_fixture(4, 5);
        assert!(matches!(
            classify_unresolved(&prepared, Some(&task_receipt(6))),
            Err(AuditStoreError::ReconciliationRequired { .. })
        ));
    }

    // ==================================================================
    // Create decision matrix
    // ==================================================================

    #[test]
    fn create_prepare_aborts_when_task_absent() {
        // For create: expected=0, replacement=0 (first snapshot).
        let prepared = prepared_fixture(0, 0);
        assert_eq!(
            classify_unresolved(&prepared, None).unwrap(),
            ReconcileDecision::Abort(AuditAbortCategory::StateNotCommitted)
        );
    }

    #[test]
    fn create_prepare_commits_when_exact_revision_zero_is_authoritative() {
        let prepared = prepared_fixture(0, 0);
        assert_eq!(
            classify_unresolved(&prepared, Some(&task_receipt(0))).unwrap(),
            ReconcileDecision::Commit
        );
    }

    #[test]
    fn create_prepare_rejects_mismatched_task_receipt() {
        let prepared = prepared_fixture(0, 0);
        let mut wrong_receipt = task_receipt(0);
        wrong_receipt.task_id = "task-ffffffff-ffff-4fff-afff-ffffffffffff".to_owned();
        assert!(matches!(
            classify_unresolved(&prepared, Some(&wrong_receipt)),
            Err(AuditStoreError::ReconciliationRequired { .. })
        ));
    }

    // ==================================================================
    // Edge cases
    // ==================================================================

    #[test]
    fn abort_on_absent_task_for_transition() {
        let prepared = prepared_fixture(4, 5);
        assert_eq!(
            classify_unresolved(&prepared, None).unwrap(),
            ReconcileDecision::Abort(AuditAbortCategory::StateNotCommitted)
        );
    }

    #[test]
    fn commit_when_authoritative_matches_replacement_even_if_expected_differs() {
        // replacement=10, expected=9, authoritative=10 → commit
        let prepared = prepared_fixture(9, 10);
        assert_eq!(
            classify_unresolved(&prepared, Some(&task_receipt(10))).unwrap(),
            ReconcileDecision::Commit
        );
    }

    #[test]
    fn abort_when_authoritative_matches_expected_even_if_close_to_replacement() {
        // replacement=10, expected=9, authoritative=9 → abort
        let prepared = prepared_fixture(9, 10);
        assert_eq!(
            classify_unresolved(&prepared, Some(&task_receipt(9))).unwrap(),
            ReconcileDecision::Abort(AuditAbortCategory::StateNotCommitted)
        );
    }
}
