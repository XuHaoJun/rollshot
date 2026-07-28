//! Physical journal record types and hash-chain validation.
//!
//! Each record is a JSON-serializable structure with a SHA-256 hash
//! over canonical fields, chaining to the previous record's hash.
//! The domain separator `b"rollshot-audit-journal-record-v1\0"` binds
//! hashes to this application.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use rollshot_agent::audit::{AuditEnvelopeV1, AuditEventId, AuditTaskStateReceiptV1};
use rollshot_agent::product_task::ProductTaskId;

// ============================================================================
// Constants
// ============================================================================

pub(crate) const JOURNAL_SCHEMA_VERSION_V1: u32 = 1;
pub(crate) const MAX_RECORD_BYTES: usize = 64 * 1024;
pub(crate) const MAX_JOURNAL_BYTES: u64 = 16 * 1024 * 1024;

const DOMAIN_SEPARATOR: &[u8] = b"rollshot-audit-journal-record-v1\0";

// ============================================================================
// Private IDs
// ============================================================================

/// Opaque audit transaction identifier. Stable across prepare/commit records.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub(crate) struct AuditTransactionId(String);

impl AuditTransactionId {
    pub(crate) fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

/// Category of abort for a prepared transaction.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum AuditAbortCategory {
    TaskStoreCommitFailed,
    StateMismatch,
    ValidationFailed,
    CallerCancelled,
    StateNotCommitted,
}

// ============================================================================
// Journal payload variants
// ============================================================================

/// Payload variant for a journal record.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "record_kind", rename_all = "snake_case")]
pub(crate) enum JournalPayloadV1 {
    Prepared(PreparedTransactionV1),
    Committed {
        transaction_id: AuditTransactionId,
        event_id: AuditEventId,
    },
    Aborted {
        transaction_id: AuditTransactionId,
        event_id: AuditEventId,
        reason: AuditAbortCategory,
    },
    Standalone {
        envelope: AuditEnvelopeV1,
    },
    Bootstrap {
        receipt: AuditTaskStateReceiptV1,
        observed_at_unix_ms: i64,
    },
}

impl JournalPayloadV1 {
    /// Returns the event ID for this payload, if present.
    pub(crate) fn event_id(&self) -> Option<&AuditEventId> {
        match self {
            Self::Prepared(p) => Some(&p.event_id),
            Self::Committed { event_id, .. } => Some(event_id),
            Self::Aborted { event_id, .. } => Some(event_id),
            Self::Standalone { envelope } => Some(envelope.event_id()),
            Self::Bootstrap { .. } => None,
        }
    }
}

/// Prepared transaction record: the envelope and revision bookkeeping
/// for a write-ahead audit transaction.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct PreparedTransactionV1 {
    pub(crate) transaction_id: AuditTransactionId,
    pub(crate) event_id: AuditEventId,
    pub(crate) envelope: AuditEnvelopeV1,
    pub(crate) expected_revision: u32,
    pub(crate) replacement_revision: u32,
    pub(crate) replacement_receipt: AuditTaskStateReceiptV1,
}

// ============================================================================
// Canonical DTO for deterministic hash computation
// ============================================================================

/// Canonical record DTO for deterministic hash computation.
///
/// Excludes `record_sha256` (the field being computed).
/// Uses serde_json with sorted object keys for canonical bytes.
#[derive(Serialize)]
struct CanonicalRecord<'a> {
    domain_separator: &'a [u8],
    schema_version: u32,
    task_id: &'a str,
    sequence: u64,
    previous_record_sha256: Option<&'a str>,
    payload: &'a JournalPayloadV1,
}

// ============================================================================
// JournalRecordV1
// ============================================================================

/// A single record in the per-task audit journal.
///
/// `record_sha256` is computed over canonical fields excluding itself.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct JournalRecordV1 {
    pub(crate) schema_version: u32,
    pub(crate) task_id: ProductTaskId,
    pub(crate) sequence: u64,
    pub(crate) previous_record_sha256: Option<String>,
    pub(crate) payload: JournalPayloadV1,
    pub(crate) record_sha256: String,
}

impl JournalRecordV1 {
    /// Build a new journal record with computed hash chain.
    ///
    /// `previous` is the preceding record in the chain, if any.
    pub(crate) fn build(
        task_id: ProductTaskId,
        sequence: u64,
        previous_record_sha256: Option<String>,
        payload: JournalPayloadV1,
    ) -> Self {
        let hash = Self::compute_hash(
            task_id.as_str(),
            sequence,
            previous_record_sha256.as_deref(),
            &payload,
        );

        Self {
            schema_version: JOURNAL_SCHEMA_VERSION_V1,
            task_id,
            sequence,
            previous_record_sha256,
            payload,
            record_sha256: hash,
        }
    }

    /// Verify this record's hash against its canonical fields.
    ///
    /// Returns `Ok(())` if the stored hash matches the computed hash.
    pub(crate) fn verify(&self, previous: Option<&JournalRecordV1>) -> Result<(), HashMismatch> {
        let computed = Self::compute_hash(
            self.task_id.as_str(),
            self.sequence,
            self.previous_record_sha256.as_deref(),
            &self.payload,
        );
        if computed != self.record_sha256 {
            return Err(HashMismatch {
                expected: computed,
                actual: self.record_sha256.clone(),
            });
        }

        // Verify chain binding.
        match (self.previous_record_sha256.as_deref(), previous) {
            (Some(expected_hash), Some(prev)) => {
                if expected_hash != prev.record_sha256 {
                    return Err(HashMismatch {
                        expected: prev.record_sha256.clone(),
                        actual: expected_hash.to_owned(),
                    });
                }
            }
            (None, None) => {}
            _ => {
                return Err(HashMismatch {
                    expected: previous
                        .map(|p| p.record_sha256.clone())
                        .unwrap_or_default(),
                    actual: self.previous_record_sha256.clone().unwrap_or_default(),
                });
            }
        }

        Ok(())
    }

    /// Compute the SHA-256 hash over canonical record fields.
    pub(crate) fn compute_hash(
        task_id: &str,
        sequence: u64,
        previous_record_sha256: Option<&str>,
        payload: &JournalPayloadV1,
    ) -> String {
        let canonical = CanonicalRecord {
            domain_separator: DOMAIN_SEPARATOR,
            schema_version: JOURNAL_SCHEMA_VERSION_V1,
            task_id,
            sequence,
            previous_record_sha256,
            payload,
        };
        let bytes =
            serde_json::to_vec(&canonical).expect("CanonicalRecord serialization infallible");
        let hash = Sha256::digest(&bytes);
        hex_encode(&hash)
    }
}

// ============================================================================
// Errors
// ============================================================================

/// Hash mismatch during chain verification.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("hash mismatch: expected={expected}, actual={actual}")]
pub(crate) struct HashMismatch {
    pub(crate) expected: String,
    pub(crate) actual: String,
}

// ============================================================================
// Helpers
// ============================================================================

pub(crate) fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

pub(crate) fn hex_valid(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit())
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use rollshot_agent::audit::AuditEventId;
    use rollshot_agent::product_task::ProductTaskId;

    // ------------------------------------------------------------------
    // Fixtures
    // ------------------------------------------------------------------

    fn task_id() -> ProductTaskId {
        ProductTaskId::parse("task-00000000-0000-4000-8000-000000000001").unwrap()
    }

    fn audit_event_id(n: u64) -> AuditEventId {
        AuditEventId::parse(format!("audit-00000000-0000-4000-8000-{n:012x}")).unwrap()
    }

    fn aborted_payload_fixture() -> JournalPayloadV1 {
        JournalPayloadV1::Aborted {
            transaction_id: AuditTransactionId::new("txn-00000000-0000-4000-8000-000000000001"),
            event_id: audit_event_id(1),
            reason: AuditAbortCategory::TaskStoreCommitFailed,
        }
    }

    fn committed_payload_fixture() -> JournalPayloadV1 {
        JournalPayloadV1::Committed {
            transaction_id: AuditTransactionId::new("txn-00000000-0000-4000-8000-000000000001"),
            event_id: audit_event_id(1),
        }
    }

    fn two_record_journal() -> Vec<JournalRecordV1> {
        let first = JournalRecordV1::build(task_id(), 0, None, aborted_payload_fixture());
        let second = JournalRecordV1::build(
            task_id(),
            1,
            Some(first.record_sha256.clone()),
            committed_payload_fixture(),
        );
        vec![first, second]
    }

    fn two_record_journal_bytes() -> Vec<u8> {
        let records = two_record_journal();
        let mut bytes = Vec::new();
        for record in &records {
            let line = serde_json::to_vec(record).unwrap();
            bytes.extend_from_slice(&line);
            bytes.push(b'\n');
        }
        bytes
    }

    fn scan_bytes(bytes: &[u8]) -> Result<Vec<JournalRecordV1>, ScanError> {
        use std::io::BufRead;
        let cursor = std::io::Cursor::new(bytes);
        let reader = std::io::BufReader::new(cursor);
        let mut records = Vec::new();
        let mut prev_hash: Option<String> = None;
        for (line_no, line_result) in reader.split(b'\n').enumerate() {
            let line = line_result.map_err(|e| ScanError::Io {
                line: line_no,
                reason: e.to_string(),
            })?;
            if line.is_empty() {
                continue;
            }
            let record: JournalRecordV1 =
                serde_json::from_slice(&line).map_err(|e| ScanError::Parse {
                    line: line_no,
                    reason: e.to_string(),
                })?;
            // Verify hash chain.
            let expected_prev = prev_hash.as_deref();
            let computed = JournalRecordV1::compute_hash(
                record.task_id.as_str(),
                record.sequence,
                record.previous_record_sha256.as_deref(),
                &record.payload,
            );
            if computed != record.record_sha256 {
                return Err(ScanError::CorruptJournal {
                    line: line_no,
                    reason: format!(
                        "hash mismatch at sequence {}: expected={}, got={}",
                        record.sequence, computed, record.record_sha256
                    ),
                });
            }
            // Verify chain binding.
            if record.previous_record_sha256.as_deref() != expected_prev {
                return Err(ScanError::CorruptJournal {
                    line: line_no,
                    reason: format!(
                        "chain break at sequence {}: expected prev={:?}, got={:?}",
                        record.sequence, expected_prev, record.previous_record_sha256
                    ),
                });
            }
            prev_hash = Some(record.record_sha256.clone());
            records.push(record);
        }
        Ok(records)
    }

    /// Replace a byte substring with another of the same length.
    fn replace_same_length_ascii(bytes: &mut [u8], from: &[u8], to: &[u8]) {
        assert_eq!(from.len(), to.len(), "replacement must be same length");
        for i in 0..=bytes.len() - from.len() {
            if &bytes[i..i + from.len()] == from {
                bytes[i..i + from.len()].copy_from_slice(to);
                return;
            }
        }
        panic!("pattern not found");
    }

    // ------------------------------------------------------------------
    // Scan error type (local to tests)
    // ------------------------------------------------------------------

    #[derive(Debug)]
    enum ScanError {
        Io { line: usize, reason: String },
        Parse { line: usize, reason: String },
        CorruptJournal { line: usize, reason: String },
    }

    // ------------------------------------------------------------------
    // Step 1: Golden hash and chain tests
    // ------------------------------------------------------------------

    #[test]
    fn first_record_has_sequence_zero_no_previous_hash_and_stable_digest() {
        let record = JournalRecordV1::build(task_id(), 0, None, aborted_payload_fixture());
        assert_eq!(record.sequence, 0);
        assert_eq!(record.previous_record_sha256, None);
        // The expected hash is computed from the canonical format.
        // Run once, capture the hash, then hardcode it here.
        assert_eq!(
            record.record_sha256,
            "ead2490272b7665cd4bfac7e1c8bc9763841f47380cac093ed4502a8b5d4e624"
        );
    }

    #[test]
    fn second_record_binds_previous_hash() {
        let first = JournalRecordV1::build(task_id(), 0, None, aborted_payload_fixture());
        let second = JournalRecordV1::build(
            task_id(),
            1,
            Some(first.record_sha256.clone()),
            committed_payload_fixture(),
        );
        assert_eq!(
            second.previous_record_sha256.as_deref(),
            Some(first.record_sha256.as_str())
        );
        assert!(second.verify(Some(&first)).is_ok());
    }

    #[test]
    fn changed_interior_payload_breaks_hash_validation() {
        let mut bytes = two_record_journal_bytes();
        // Replace a string value in the JSON that still deserializes
        // but changes the hash. The event_id contains "audit-" prefix.
        // Changing part of the hex digits preserves valid JSON.
        replace_same_length_ascii(&mut bytes, b"000000000001", b"111111111111");
        assert!(matches!(
            scan_bytes(&bytes),
            Err(ScanError::CorruptJournal { .. })
        ));
    }
}
