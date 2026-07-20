//! Integration tests for per-output publish state persistence and cancellation.

use std::path::Path;

use rollshot_action::project::{
    create_project, load_publish_state, write_publish_state, EnabledOutputs, ProjectSnapshot,
    PublishCancellation, PublishCancelled, PublishFreshness, PublishOutputKind, PublishStateLoad,
    PublishStateV1, PublishedOutputV1, SnapshotFrame, SnapshotFramePayload,
};
use rollshot_action::{
    CandidateKind, CaptureRegion, DetectReason, InputCapability, InputSourceKind,
};
use std::sync::Arc;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn committed_project() -> (tempfile::TempDir, std::path::PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("guide.rollshot-guide");
    let snap = ProjectSnapshot {
        base_revision: None,
        title: "Test".into(),
        capture_region: CaptureRegion {
            x: 0,
            y: 0,
            width: 640,
            height: 480,
        },
        input_source: InputSourceKind::VisualOnly,
        input_capability: InputCapability::SemanticEvents,
        enabled_outputs: EnabledOutputs::default(),
        frames: vec![SnapshotFrame {
            id: 1,
            at_ms: 100,
            payload: SnapshotFramePayload::Pixels(Arc::new(image::RgbaImage::from_pixel(
                640,
                480,
                image::Rgba([10, 20, 30, 255]),
            ))),
        }],
        steps: vec![rollshot_action::project::ProjectStep {
            id: rollshot_action::project::ProjectStepId(1),
            order: 1,
            title: "Step 1".into(),
            caption: None,
            kind: CandidateKind::Click,
            reason: DetectReason::ClickConfirmed,
            at_ms: 150,
            keyframe: 1,
            nearby: vec![1],
            annotations: None,
        }],
        import_warnings: Vec::new(),
    };
    create_project(&snap, &root).unwrap();
    (dir, root)
}

fn write_state(root: &Path, state: &PublishStateV1) {
    write_publish_state(root, state).unwrap();
}

fn write_complete_core_bundle(root: &Path) {
    let publish = root.join("publish");
    std::fs::write(publish.join("index.html"), b"<html></html>").unwrap();
    std::fs::write(publish.join("steps.md"), b"# Steps\n").unwrap();
    std::fs::write(
        publish.join("session.json"),
        r#"{"schema_version":1,"title":"T","region":{"x":0,"y":0,"width":640,"height":480},"input_source":"visual-only","input_capability":"semantic-events","steps":[]}"#,
    )
    .unwrap();
}

fn state_with_success(kind: PublishOutputKind, revision: u64) -> PublishStateV1 {
    let mut state = PublishStateV1::default();
    state.outputs.insert(kind, PublishedOutputV1::new(revision));
    state
}

// ---------------------------------------------------------------------------
// Tests: missing and corrupt state
// ---------------------------------------------------------------------------

#[test]
fn missing_publish_state_is_unavailable() {
    let (_dir, root) = committed_project();
    let loaded = load_publish_state(&root);
    assert!(matches!(loaded, PublishStateLoad::Unavailable));
}

#[test]
fn corrupt_publish_state_is_non_fatal_and_all_stale() {
    let (_dir, root) = committed_project();
    std::fs::write(root.join("publish-state.json"), b"{").unwrap();

    let loaded = load_publish_state(&root);
    assert!(matches!(loaded, PublishStateLoad::Unavailable));
    for &kind in PublishOutputKind::ALL {
        assert_eq!(loaded.freshness(kind, 4), PublishFreshness::Stale);
    }
}

// ---------------------------------------------------------------------------
// Tests: unknown fields and unsupported schema
// ---------------------------------------------------------------------------

#[test]
fn unknown_fields_in_publish_state_are_rejected() {
    let (_dir, root) = committed_project();
    std::fs::write(
        root.join("publish-state.json"),
        r#"{"schema_version":1,"outputs":{},"surprise":true}"#,
    )
    .unwrap();

    let loaded = load_publish_state(&root);
    assert!(matches!(loaded, PublishStateLoad::Unavailable));
}

#[test]
fn unsupported_schema_version_is_rejected() {
    let (_dir, root) = committed_project();
    let state = state_with_success(PublishOutputKind::Core, 1);
    write_state(&root, &state);

    let mut raw: serde_json::Value =
        serde_json::from_slice(&std::fs::read(root.join("publish-state.json")).unwrap()).unwrap();
    raw["schema_version"] = serde_json::json!(99);
    std::fs::write(
        root.join("publish-state.json"),
        serde_json::to_vec_pretty(&raw).unwrap(),
    )
    .unwrap();

    let loaded = load_publish_state(&root);
    assert!(matches!(loaded, PublishStateLoad::Unavailable));
}

// ---------------------------------------------------------------------------
// Tests: freshness and revision
// ---------------------------------------------------------------------------

#[test]
fn freshness_requires_the_exact_saved_revision() {
    let (_dir, root) = committed_project();
    write_complete_core_bundle(&root);
    write_state(&root, &state_with_success(PublishOutputKind::Core, 3));

    let loaded = load_publish_state(&root);
    assert_eq!(
        loaded.freshness(PublishOutputKind::Core, 3),
        PublishFreshness::Current
    );
    assert_eq!(
        loaded.freshness(PublishOutputKind::Core, 4),
        PublishFreshness::Stale
    );
}

#[test]
fn missing_output_derivative_is_stale_even_at_matching_revision() {
    let (_dir, root) = committed_project();
    write_state(&root, &state_with_success(PublishOutputKind::Core, 1));

    let loaded = load_publish_state(&root);
    assert_eq!(
        loaded.freshness(PublishOutputKind::Core, 1),
        PublishFreshness::Stale
    );
}

#[test]
fn symlinked_derivative_is_stale() {
    let (_dir, root) = committed_project();
    write_complete_core_bundle(&root);

    let index_path = root.join("publish/index.html");
    std::fs::remove_file(&index_path).unwrap();
    std::os::unix::fs::symlink("/dev/null", &index_path).unwrap();

    write_state(&root, &state_with_success(PublishOutputKind::Core, 1));

    let loaded = load_publish_state(&root);
    assert_eq!(
        loaded.freshness(PublishOutputKind::Core, 1),
        PublishFreshness::Stale
    );
}

#[test]
fn incomplete_core_viewer_tree_is_stale() {
    let (_dir, root) = committed_project();
    let publish = root.join("publish");
    std::fs::write(publish.join("index.html"), b"<html></html>").unwrap();
    // Missing steps.md and session.json
    write_state(&root, &state_with_success(PublishOutputKind::Core, 1));

    let loaded = load_publish_state(&root);
    assert_eq!(
        loaded.freshness(PublishOutputKind::Core, 1),
        PublishFreshness::Stale
    );
}

#[test]
fn mixed_current_and_stale_outputs() {
    let (_dir, root) = committed_project();
    write_complete_core_bundle(&root);

    let mut state = PublishStateV1::default();
    state
        .outputs
        .insert(PublishOutputKind::Core, PublishedOutputV1::new(1));
    state
        .outputs
        .insert(PublishOutputKind::Storyboard, PublishedOutputV1::new(1));
    write_state(&root, &state);

    let loaded = load_publish_state(&root);
    assert_eq!(
        loaded.freshness(PublishOutputKind::Core, 1),
        PublishFreshness::Current
    );
    assert_eq!(
        loaded.freshness(PublishOutputKind::Storyboard, 1),
        PublishFreshness::Stale
    );
}

// ---------------------------------------------------------------------------
// Tests: serialization round-trip and unknown output kinds
// ---------------------------------------------------------------------------

#[test]
fn serialization_round_trip() {
    let mut state = PublishStateV1::default();
    state
        .outputs
        .insert(PublishOutputKind::Core, PublishedOutputV1::new(5));
    state
        .outputs
        .insert(PublishOutputKind::Gif, PublishedOutputV1::new(3));

    let json = serde_json::to_string(&state).unwrap();
    let parsed: PublishStateV1 = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.outputs.len(), 2);
    assert_eq!(
        parsed.outputs[&PublishOutputKind::Core].last_successful_revision,
        5
    );
}

#[test]
fn unknown_output_kind_in_state_is_rejected() {
    let (_dir, root) = committed_project();
    std::fs::write(
        root.join("publish-state.json"),
        r#"{"schema_version":1,"outputs":{"core":{"last_successful_revision":1},"unknown_kind":{"last_successful_revision":2}}}"#,
    )
    .unwrap();

    let loaded = load_publish_state(&root);
    // Unknown enum variant in BTreeMap key → serde rejects the whole file
    assert!(matches!(loaded, PublishStateLoad::Unavailable));
}

// ---------------------------------------------------------------------------
// Tests: atomic rewrite
// ---------------------------------------------------------------------------

#[test]
fn write_publish_state_is_atomic_rewrite() {
    let (_dir, root) = committed_project();

    let mut state = PublishStateV1::default();
    state
        .outputs
        .insert(PublishOutputKind::Core, PublishedOutputV1::new(1));
    write_state(&root, &state);

    state
        .outputs
        .insert(PublishOutputKind::Gif, PublishedOutputV1::new(2));
    write_state(&root, &state);

    let bytes = std::fs::read(root.join("publish-state.json")).unwrap();
    let parsed: PublishStateV1 = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(parsed.outputs.len(), 2);
    assert_eq!(
        parsed.outputs[&PublishOutputKind::Core].last_successful_revision,
        1
    );
    assert_eq!(
        parsed.outputs[&PublishOutputKind::Gif].last_successful_revision,
        2
    );
}

#[test]
fn write_failure_preserves_prior_bytes() {
    let (_dir, root) = committed_project();

    let mut state = PublishStateV1::default();
    state
        .outputs
        .insert(PublishOutputKind::Core, PublishedOutputV1::new(1));
    write_state(&root, &state);

    let before = std::fs::read(root.join("publish-state.json")).unwrap();

    // Make the directory read-only to cause a write failure
    // (only works as non-root; skip if we can't)
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let orig = std::fs::metadata(&root).unwrap().permissions();
        std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o555)).unwrap();

        let mut state2 = PublishStateV1::default();
        state2
            .outputs
            .insert(PublishOutputKind::Core, PublishedOutputV1::new(2));
        let result = write_publish_state(&root, &state2);

        std::fs::set_permissions(&root, orig).unwrap();

        if result.is_err() {
            let after = std::fs::read(root.join("publish-state.json")).unwrap();
            assert_eq!(before, after);
        }
        // If running as root, the write may succeed — that's OK, skip the assertion.
    }
}

// ---------------------------------------------------------------------------
// Tests: cancellation
// ---------------------------------------------------------------------------

#[test]
fn cancellation_starts_not_cancelled() {
    let cancel = PublishCancellation::new();
    assert!(!cancel.is_cancelled());
    assert!(cancel.check().is_ok());
}

#[test]
fn cancellation_signal_propagates() {
    let cancel = PublishCancellation::new();
    cancel.cancel();
    assert!(cancel.is_cancelled());
    assert!(cancel.check().is_err());
}

#[test]
fn cancellation_is_idempotent() {
    let cancel = PublishCancellation::new();
    cancel.cancel();
    cancel.cancel();
    cancel.cancel();
    assert!(cancel.is_cancelled());
}

#[test]
fn publish_cancelled_is_zero_sized() {
    assert_eq!(std::mem::size_of::<PublishCancelled>(), 0);
}

// ---------------------------------------------------------------------------
// Tests: Core keyframe reference validation
// ---------------------------------------------------------------------------

fn session_manifest_with_keyframes(files: &[&str]) -> String {
    let steps: Vec<serde_json::Value> = files
        .iter()
        .enumerate()
        .map(|(i, f)| {
            serde_json::json!({
                "index": i + 1,
                "title": format!("Step {}", i + 1),
                "kind": "click",
                "reason": "click-confirmed",
                "at_ms": 100,
                "keyframe_file": f,
                "hotspots": []
            })
        })
        .collect();
    serde_json::json!({
        "schema_version": 1,
        "title": "T",
        "region": { "x": 0, "y": 0, "width": 640, "height": 480 },
        "input_source": "visual-only",
        "input_capability": "semantic-events",
        "steps": steps
    })
    .to_string()
}

fn write_core_bundle_with_keyframes(root: &Path, keyframe_files: &[&str]) {
    let publish = root.join("publish");
    std::fs::write(publish.join("index.html"), b"<html></html>").unwrap();
    std::fs::write(publish.join("steps.md"), b"# Steps\n").unwrap();
    std::fs::write(
        publish.join("session.json"),
        session_manifest_with_keyframes(keyframe_files),
    )
    .unwrap();
}

#[test]
fn core_bundle_missing_keyframe_is_stale() {
    let (_dir, root) = committed_project();
    write_core_bundle_with_keyframes(&root, &["keyframes/001.png"]);
    // Don't create the keyframe file
    write_state(&root, &state_with_success(PublishOutputKind::Core, 1));

    let loaded = load_publish_state(&root);
    assert_eq!(
        loaded.freshness(PublishOutputKind::Core, 1),
        PublishFreshness::Stale
    );
}

#[test]
fn core_bundle_symlinked_keyframe_is_stale() {
    let (_dir, root) = committed_project();
    let keyframes = root.join("publish/keyframes");
    std::fs::create_dir_all(&keyframes).unwrap();
    std::os::unix::fs::symlink("/dev/null", keyframes.join("001.png")).unwrap();

    write_core_bundle_with_keyframes(&root, &["keyframes/001.png"]);
    write_state(&root, &state_with_success(PublishOutputKind::Core, 1));

    let loaded = load_publish_state(&root);
    assert_eq!(
        loaded.freshness(PublishOutputKind::Core, 1),
        PublishFreshness::Stale
    );
}

#[test]
fn core_bundle_absolute_keyframe_reference_is_rejected() {
    let (_dir, root) = committed_project();
    let keyframes = root.join("publish/keyframes");
    std::fs::create_dir_all(&keyframes).unwrap();
    std::fs::write(keyframes.join("001.png"), b"fake").unwrap();

    write_core_bundle_with_keyframes(&root, &["/etc/passwd"]);
    write_state(&root, &state_with_success(PublishOutputKind::Core, 1));

    let loaded = load_publish_state(&root);
    assert_eq!(
        loaded.freshness(PublishOutputKind::Core, 1),
        PublishFreshness::Stale
    );
}

#[test]
fn core_bundle_parent_traversing_keyframe_reference_is_rejected() {
    let (_dir, root) = committed_project();
    write_core_bundle_with_keyframes(&root, &["../escape.png"]);
    write_state(&root, &state_with_success(PublishOutputKind::Core, 1));

    let loaded = load_publish_state(&root);
    assert_eq!(
        loaded.freshness(PublishOutputKind::Core, 1),
        PublishFreshness::Stale
    );
}

#[test]
fn core_bundle_non_keyframes_prefix_reference_is_rejected() {
    let (_dir, root) = committed_project();
    write_core_bundle_with_keyframes(&root, &["other/001.png"]);
    write_state(&root, &state_with_success(PublishOutputKind::Core, 1));

    let loaded = load_publish_state(&root);
    assert_eq!(
        loaded.freshness(PublishOutputKind::Core, 1),
        PublishFreshness::Stale
    );
}

#[test]
fn core_bundle_non_png_keyframe_reference_is_rejected() {
    let (_dir, root) = committed_project();
    write_core_bundle_with_keyframes(&root, &["keyframes/001.jpg"]);
    write_state(&root, &state_with_success(PublishOutputKind::Core, 1));

    let loaded = load_publish_state(&root);
    assert_eq!(
        loaded.freshness(PublishOutputKind::Core, 1),
        PublishFreshness::Stale
    );
}

#[test]
fn core_bundle_duplicate_keyframe_references_are_rejected() {
    let (_dir, root) = committed_project();
    let keyframes = root.join("publish/keyframes");
    std::fs::create_dir_all(&keyframes).unwrap();
    std::fs::write(keyframes.join("001.png"), b"fake").unwrap();

    write_core_bundle_with_keyframes(&root, &["keyframes/001.png", "keyframes/001.png"]);
    write_state(&root, &state_with_success(PublishOutputKind::Core, 1));

    let loaded = load_publish_state(&root);
    assert_eq!(
        loaded.freshness(PublishOutputKind::Core, 1),
        PublishFreshness::Stale
    );
}

#[test]
fn core_bundle_with_valid_keyframes_is_current() {
    let (_dir, root) = committed_project();
    let keyframes = root.join("publish/keyframes");
    std::fs::create_dir_all(&keyframes).unwrap();
    std::fs::write(keyframes.join("001.png"), b"fake1").unwrap();
    std::fs::write(keyframes.join("002.png"), b"fake2").unwrap();

    write_core_bundle_with_keyframes(&root, &["keyframes/001.png", "keyframes/002.png"]);
    write_state(&root, &state_with_success(PublishOutputKind::Core, 1));

    let loaded = load_publish_state(&root);
    assert_eq!(
        loaded.freshness(PublishOutputKind::Core, 1),
        PublishFreshness::Current
    );
}
