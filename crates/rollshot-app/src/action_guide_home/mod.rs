pub(crate) mod recent;
pub(crate) mod update;
pub(crate) mod video_import;
pub(crate) mod view;

#[allow(unused_imports)]
pub use update::{
    legacy_reader_entrypoint, ActionGuideHome, ActionGuideIntent, Effect, Message,
    RecordPreflight, RecordPreflightPhase, SelectedDirectoryKind,
};
#[allow(unused_imports)]
pub use view::view;

pub(crate) fn cleanup_stale_import_scratch() {
    let scratch_parent = std::env::temp_dir().join("rollshot/import");
    let before = std::fs::read_dir(&scratch_parent)
        .ok()
        .map(|entries| {
            entries
                .flatten()
                .filter(|e| e.file_name().to_string_lossy().starts_with("import-"))
                .count()
        })
        .unwrap_or(0);
    rollshot_action::cleanup_stale_import_scratch(&scratch_parent);
    let after = std::fs::read_dir(&scratch_parent)
        .ok()
        .map(|entries| {
            entries
                .flatten()
                .filter(|e| e.file_name().to_string_lossy().starts_with("import-"))
                .count()
        })
        .unwrap_or(0);
    let removed = before.saturating_sub(after);
    if removed > 0 {
        tracing::info!(
            target: "rollshot::app",
            removed_count = removed,
            "stale import scratch cleaned up at startup"
        );
    }
}
