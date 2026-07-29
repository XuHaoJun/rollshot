//! Process-wide agent task persistence.
//!
//! Exactly one [`TaskStore`] exists per process. `TaskStore::acquire_lock`
//! takes a blocking fs4 exclusive lock per operation, and two instances in one
//! process hold distinct file descriptors that flock treats as unrelated
//! holders: they block each other, and nested acquisition self-deadlocks.
//!
//! This module is unconditional. Only Action Guide task-kind construction
//! sites are gated on the `action-guide` feature.

// Carried over from the previous parent (`result_workspace::workbench`),
// which suppressed these same lints for this scaffolding: several items
// (failpoints, some error variants) are exercised only by tests until later
// tasks finish wiring the store into more call sites, and the journal
// payload enum has one deliberately larger variant.
#![allow(dead_code, clippy::large_enum_variant)]

pub mod audit_store;
pub mod task_store;

// `audit_store`'s items are all `pub(crate)`; `pub use` of a `pub(crate)` item
// is E0364/E0365, so these re-exports must be crate-visible too.
#[allow(unused_imports)]
pub(crate) use audit_store::{AuditJournal, AuditStoreError, TaskAuditSink};
#[allow(unused_imports)]
pub use task_store::{
    Failpoint, StoreCommitOutcome, TaskStore, TaskStoreContinuitySource, TaskStoreError,
};

/// Open the single process-wide task store.
pub fn open_process_store(
    config_dir: &std::path::Path,
) -> Result<std::sync::Arc<TaskStore>, TaskStoreError> {
    let store = TaskStore::open(config_dir)?;
    tracing::info!(
        target: "rollshot::app::agent_store",
        tasks_dir = %store.tasks_dir().display(),
        "process task store opened"
    );
    Ok(std::sync::Arc::new(store))
}

#[cfg(test)]
mod placement_tests {
    #[test]
    fn store_module_is_reachable_without_action_guide() {
        // The store is unconditional: only Action Guide task-kind construction
        // sites are feature-gated. This test exists in both feature configs.
        let dir = tempfile::tempdir().unwrap();
        let store = super::open_process_store(dir.path()).unwrap();

        assert!(store.tasks_dir().exists());
    }
}
