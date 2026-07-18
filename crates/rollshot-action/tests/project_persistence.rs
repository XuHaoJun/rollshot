//! End-to-end integration tests proving the public persistence contract.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;

use image::{Rgba, RgbaImage};

use rollshot_action::project::{
    create_project, load_project, save_project, save_project_as, EnabledOutputs,
    PersistedStepAnnotations, ProjectCommit, ProjectSnapshot, ProjectStep, ProjectStepId,
    SnapshotFrame, SnapshotFramePayload,
};
use rollshot_action::{
    CandidateKind, CaptureRegion, DetectReason, InputCapability, InputSourceKind,
};
use rollshot_image_document::{
    Annotation, AnnotationId, FreehandKind, ImagePoint, ImageRect, ShapeKind, TwoPointKind,
};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

const REGION: CaptureRegion = CaptureRegion {
    x: 10,
    y: 20,
    width: 640,
    height: 480,
};

fn pixel(w: u32, h: u32) -> Arc<RgbaImage> {
    Arc::new(RgbaImage::from_pixel(w, h, Rgba([42, 43, 44, 255])))
}

/// All seven annotation variants for a 640×480 image.
fn all_annotations() -> Vec<Annotation> {
    vec![
        // 1. TwoPoint / Arrow
        Annotation::two_point(
            AnnotationId(1),
            TwoPointKind::Arrow,
            ImagePoint::new(10.0, 20.0),
            ImagePoint::new(200.0, 150.0),
        ),
        // 2. NumberCallout
        Annotation::number_callout(
            AnnotationId(2),
            1,
            ImagePoint::new(250.0, 300.0),
            ImagePoint::new(260.0, 290.0),
        ),
        // 3. TextNote
        Annotation::text_note(
            AnnotationId(3),
            ImagePoint::new(300.0, 100.0),
            "Hello annotation".into(),
        ),
        // 4. OpaqueRedaction
        Annotation::opaque_redaction(AnnotationId(4), ImageRect::new(50.0, 50.0, 80.0, 40.0)),
        // 5. Shape / Rectangle
        Annotation::shape(
            AnnotationId(5),
            ShapeKind::Rectangle,
            ImageRect::new(400.0, 200.0, 60.0, 60.0),
        ),
        // 6. Freehand / Pen
        Annotation::freehand(
            AnnotationId(6),
            FreehandKind::Pen,
            vec![
                ImagePoint::new(0.0, 0.0),
                ImagePoint::new(10.0, 5.0),
                ImagePoint::new(20.0, 15.0),
            ],
        ),
        // 7. Pixelate
        Annotation::pixelate(AnnotationId(7), ImageRect::new(100.0, 100.0, 50.0, 50.0), 8),
    ]
}

fn explanations_for(annotations: &[Annotation]) -> BTreeMap<AnnotationId, String> {
    let mut m = BTreeMap::new();
    // Explanation on the first annotation
    m.insert(annotations[0].id(), "Explains the arrow".into());
    m
}

fn build_initial_snapshot(pixel_a: Arc<RgbaImage>, pixel_b: Arc<RgbaImage>) -> ProjectSnapshot {
    let annotations = all_annotations();
    ProjectSnapshot {
        base_revision: None,
        title: "E2E Guide".into(),
        capture_region: REGION,
        input_source: InputSourceKind::LinuxEvdev,
        input_capability: InputCapability::SemanticEvents,
        enabled_outputs: EnabledOutputs {
            storyboard: true,
            gif: false,
            mp4: true,
        },
        frames: vec![
            SnapshotFrame {
                id: 1,
                at_ms: 100,
                payload: SnapshotFramePayload::Pixels(pixel_a),
            },
            SnapshotFrame {
                id: 2,
                at_ms: 200,
                payload: SnapshotFramePayload::Pixels(pixel_b),
            },
        ],
        steps: vec![
            ProjectStep {
                id: ProjectStepId(1),
                order: 1,
                title: "Click the button".into(),
                caption: Some("Press OK to continue".into()),
                kind: CandidateKind::Click,
                reason: DetectReason::ClickConfirmed,
                at_ms: 150,
                keyframe: 1,
                nearby: vec![1, 2],
                annotations: Some(PersistedStepAnnotations {
                    annotations,
                    explanations: explanations_for(&all_annotations()),
                }),
            },
            ProjectStep {
                id: ProjectStepId(2),
                order: 2,
                title: "Type text".into(),
                caption: None,
                kind: CandidateKind::Typing,
                reason: DetectReason::TypingSettled,
                at_ms: 250,
                keyframe: 2,
                nearby: vec![1, 2],
                annotations: None,
            },
        ],
    }
}

fn asset_count(root: &Path) -> usize {
    std::fs::read_dir(root.join("assets").join("frames"))
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.file_name()
                .to_str()
                .map(|n| n.ends_with(".png"))
                .unwrap_or(false)
        })
        .count()
}

fn read_manifest_json(root: &Path) -> serde_json::Value {
    let bytes = std::fs::read(root.join("project.json")).unwrap();
    serde_json::from_slice(&bytes).unwrap()
}

// ---------------------------------------------------------------------------
// Step 1: Round-trip fixture
// ---------------------------------------------------------------------------

#[test]
fn round_trip_full_public_api_contract() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("guide.rollshot-guide");

    // Two frames with identical pixel content
    let shared_pixels = pixel(640, 480);
    let snap = build_initial_snapshot(shared_pixels.clone(), shared_pixels);

    // ---- Create (revision 1) ----
    let commit1 = create_project(&snap, &root).unwrap();
    assert_eq!(commit1.manifest.revision, 1);

    // Shared PNG bytes → one asset file
    assert_eq!(
        commit1.manifest.frames[0].sha256, commit1.manifest.frames[1].sha256,
        "duplicate pixels must produce identical sha256"
    );
    assert_eq!(
        asset_count(&root),
        1,
        "shared pixels → one asset file on disk"
    );

    // Exact metadata survives
    assert_eq!(commit1.manifest.title, "E2E Guide");
    assert_eq!(commit1.manifest.capture_region, REGION);
    assert_eq!(commit1.manifest.input_source, InputSourceKind::LinuxEvdev);
    assert_eq!(
        commit1.manifest.input_capability,
        InputCapability::SemanticEvents
    );
    assert!(commit1.manifest.enabled_outputs.storyboard);
    assert!(!commit1.manifest.enabled_outputs.gif);
    assert!(commit1.manifest.enabled_outputs.mp4);

    // Step metadata
    let s1 = &commit1.manifest.steps[0];
    assert_eq!(s1.title, "Click the button");
    assert_eq!(s1.caption.as_deref(), Some("Press OK to continue"));
    assert_eq!(s1.kind, CandidateKind::Click);
    assert_eq!(s1.reason, DetectReason::ClickConfirmed);
    assert_eq!(s1.at_ms, 150);
    assert_eq!(s1.keyframe, 1);
    assert_eq!(s1.nearby, vec![1, 2]);

    let s2 = &commit1.manifest.steps[1];
    assert_eq!(s2.title, "Type text");
    assert_eq!(s2.caption, None);
    assert_eq!(s2.kind, CandidateKind::Typing);
    assert_eq!(s2.keyframe, 2);
    assert_eq!(s2.nearby, vec![1, 2]);

    // Annotations and explanation IDs survive
    let ann = s1.annotations.as_ref().unwrap();
    assert_eq!(ann.annotations.len(), 7, "all seven variants persisted");
    assert_eq!(ann.annotations[0].id(), AnnotationId(1));
    assert_eq!(ann.annotations[1].id(), AnnotationId(2));
    assert_eq!(ann.annotations[6].id(), AnnotationId(7));
    assert!(
        ann.explanations.contains_key(&AnnotationId(1)),
        "explanation for annotation 1 survives"
    );

    // No undo/redo state in project.json
    let json = read_manifest_json(&root);
    assert!(
        json.get("undo_stack").is_none(),
        "no undo_stack in project.json"
    );
    assert!(
        json.get("redo_stack").is_none(),
        "no redo_stack in project.json"
    );
    assert!(json.get("history").is_none(), "no history in project.json");

    // Frame paths are derived, not present as fields
    for frame_json in json["frames"].as_array().unwrap() {
        assert!(
            frame_json.get("path").is_none(),
            "frame path must be derived, not a persisted field"
        );
    }

    // ---- Close and load ----
    drop(commit1);
    let loaded1 = load_project(&root).unwrap();
    assert_eq!(loaded1.manifest.revision, 1);
    assert_eq!(loaded1.manifest.title, "E2E Guide");
    assert_eq!(loaded1.manifest.steps.len(), 2);
    assert_eq!(
        loaded1.manifest.steps[0]
            .annotations
            .as_ref()
            .unwrap()
            .annotations
            .len(),
        7
    );

    // ---- Build Save snapshot using ExistingAsset payloads ----
    let existing_frame1 = &loaded1.manifest.frames[0];
    let existing_frame2 = &loaded1.manifest.frames[1];

    // Change step 1 keyframe to frame 2, add caption to step 2
    let mut save_snap = ProjectSnapshot {
        base_revision: Some(1),
        title: "E2E Guide Updated".into(),
        capture_region: REGION,
        input_source: InputSourceKind::LinuxEvdev,
        input_capability: InputCapability::SemanticEvents,
        enabled_outputs: EnabledOutputs {
            storyboard: true,
            gif: false,
            mp4: true,
        },
        frames: vec![
            SnapshotFrame {
                id: 1,
                at_ms: 100,
                payload: SnapshotFramePayload::ExistingAsset {
                    project_root: root.clone(),
                    sha256: existing_frame1.sha256.clone(),
                    width: existing_frame1.width,
                    height: existing_frame1.height,
                },
            },
            SnapshotFrame {
                id: 2,
                at_ms: 200,
                payload: SnapshotFramePayload::ExistingAsset {
                    project_root: root.clone(),
                    sha256: existing_frame2.sha256.clone(),
                    width: existing_frame2.width,
                    height: existing_frame2.height,
                },
            },
        ],
        steps: vec![
            ProjectStep {
                id: ProjectStepId(1),
                order: 1,
                title: "Click the button".into(),
                caption: Some("Press OK to continue".into()),
                kind: CandidateKind::Click,
                reason: DetectReason::ClickConfirmed,
                at_ms: 150,
                keyframe: 2, // changed keyframe
                nearby: vec![1, 2],
                annotations: loaded1.manifest.steps[0].annotations.clone(),
            },
            ProjectStep {
                id: ProjectStepId(2),
                order: 2,
                title: "Type text".into(),
                caption: Some("New caption".into()),
                kind: CandidateKind::Typing,
                reason: DetectReason::TypingSettled,
                at_ms: 250,
                keyframe: 2,
                nearby: vec![1, 2],
                annotations: None,
            },
        ],
    };

    // ---- Save (revision 2) ----
    let commit2 = save_project(&save_snap, &root).unwrap();
    assert_eq!(commit2.manifest.revision, 2);
    assert_eq!(commit2.manifest.title, "E2E Guide Updated");

    // Still one asset file (same content)
    assert_eq!(asset_count(&root), 1);

    // Changed keyframe survived
    assert_eq!(commit2.manifest.steps[0].keyframe, 2);

    // New caption survived
    assert_eq!(
        commit2.manifest.steps[1].caption.as_deref(),
        Some("New caption")
    );

    // Nearby order preserved
    assert_eq!(commit2.manifest.steps[0].nearby, vec![1, 2]);

    // ---- Load again ----
    let loaded2 = load_project(&root).unwrap();
    assert_eq!(loaded2.manifest.revision, 2);
    assert_eq!(loaded2.manifest.title, "E2E Guide Updated");
    assert_eq!(
        loaded2.manifest.steps[0]
            .annotations
            .as_ref()
            .unwrap()
            .annotations
            .len(),
        7
    );
    assert_eq!(
        loaded2.manifest.steps[0]
            .annotations
            .as_ref()
            .unwrap()
            .explanations
            .len(),
        1
    );

    // ---- Save As to new location ----
    let root_as = dir.path().join("copy.rollshot-guide");
    save_snap.base_revision = None;
    let commit_as = save_project_as(&save_snap, &root_as).unwrap();
    assert_eq!(commit_as.manifest.revision, 1, "Save As resets to 1");
    assert_eq!(asset_count(&root_as), 1);

    // Verify asset file is identical
    let orig_asset = root
        .join("assets/frames")
        .join(format!("{}.png", existing_frame1.sha256));
    let copy_asset = root_as
        .join("assets/frames")
        .join(format!("{}.png", existing_frame1.sha256));
    assert_eq!(
        std::fs::read(&orig_asset).unwrap(),
        std::fs::read(&copy_asset).unwrap(),
        "Save As preserves exact asset bytes"
    );
}

// ---------------------------------------------------------------------------
// Step 2: Filesystem damage tests
// ---------------------------------------------------------------------------

fn setup_project(root: &Path) -> ProjectCommit {
    let pixels = pixel(640, 480);
    let snap = build_initial_snapshot(pixels.clone(), pixels);
    create_project(&snap, root).unwrap()
}

#[test]
fn load_missing_directory() {
    let dir = tempfile::tempdir().unwrap();
    let missing = dir.path().join("does-not-exist");
    let err = load_project(&missing).unwrap_err();
    assert_eq!(err.category(), "io");
}

#[test]
fn load_directory_without_project_json() {
    let dir = tempfile::tempdir().unwrap();
    let empty = dir.path().join("empty-dir");
    std::fs::create_dir_all(&empty).unwrap();
    let err = load_project(&empty).unwrap_err();
    assert_eq!(err.category(), "io");
}

#[test]
fn load_truncated_project_json() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("guide.rollshot-guide");
    setup_project(&root);

    // Truncate to first 10 bytes
    let path = root.join("project.json");
    let full = std::fs::read(&path).unwrap();
    std::fs::write(&path, &full[..10]).unwrap();

    let err = load_project(&root).unwrap_err();
    assert_eq!(err.category(), "invalid-json");
}

#[test]
fn load_unknown_field_in_project_json() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("guide.rollshot-guide");
    setup_project(&root);

    let mut json = read_manifest_json(&root);
    json["surprise_field"] = serde_json::Value::Bool(true);
    std::fs::write(
        root.join("project.json"),
        serde_json::to_vec_pretty(&json).unwrap(),
    )
    .unwrap();

    let err = load_project(&root).unwrap_err();
    assert_eq!(err.category(), "invalid-json");
}

#[test]
fn load_mutated_png_byte() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("guide.rollshot-guide");
    let commit = setup_project(&root);

    let sha = &commit.manifest.frames[0].sha256;
    let path = root.join("assets/frames").join(format!("{sha}.png"));
    let mut data = std::fs::read(&path).unwrap();
    // Mutate a byte in the IDAT payload (after IHDR)
    if data.len() > 40 {
        data[35] ^= 0xFF;
    }
    std::fs::write(&path, &data).unwrap();

    let err = load_project(&root).unwrap_err();
    assert_eq!(err.category(), "invalid-asset");
}

#[test]
fn load_png_replaced_with_invalid_header() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("guide.rollshot-guide");
    let commit = setup_project(&root);

    let sha = &commit.manifest.frames[0].sha256;
    let path = root.join("assets/frames").join(format!("{sha}.png"));
    std::fs::write(&path, b"this is not a PNG file at all").unwrap();

    let err = load_project(&root).unwrap_err();
    // Could be invalid-asset or io depending on how open_project_asset fails
    assert!(
        err.category() == "invalid-asset" || err.category() == "io",
        "expected invalid-asset or io, got: {}",
        err.category()
    );
}

#[test]
fn load_assets_dir_replaced_with_symlink() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("guide.rollshot-guide");
    setup_project(&root);

    let external = dir.path().join("external_assets");
    std::fs::create_dir_all(external.join("frames")).unwrap();
    std::fs::remove_dir_all(root.join("assets")).unwrap();
    std::os::unix::fs::symlink(&external, root.join("assets")).unwrap();

    let err = load_project(&root).unwrap_err();
    // open_project_asset uses NOFOLLOW at every level, so a symlink produces
    // ELOOP → io unconditionally (never reaches the invalid-asset stat check).
    assert_eq!(err.category(), "io");
}

#[test]
fn load_frames_dir_replaced_with_symlink() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("guide.rollshot-guide");
    setup_project(&root);

    let external = dir.path().join("external_frames");
    std::fs::create_dir_all(&external).unwrap();
    std::fs::remove_dir_all(root.join("assets/frames")).unwrap();
    std::os::unix::fs::symlink(&external, root.join("assets/frames")).unwrap();

    let err = load_project(&root).unwrap_err();
    // open_project_asset uses NOFOLLOW at every level, so a symlink produces
    // ELOOP → io unconditionally (never reaches the invalid-asset stat check).
    assert_eq!(err.category(), "io");
}

#[test]
fn load_png_replaced_with_symlink() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("guide.rollshot-guide");
    let commit = setup_project(&root);

    let sha = &commit.manifest.frames[0].sha256;
    let asset_path = root.join("assets/frames").join(format!("{sha}.png"));
    let target = root.join("assets/frames/external.png");
    std::fs::write(&target, b"external content").unwrap();
    std::fs::remove_file(&asset_path).unwrap();
    std::os::unix::fs::symlink(&target, &asset_path).unwrap();

    let err = load_project(&root).unwrap_err();
    // open_project_asset uses NOFOLLOW at every level, so a symlink produces
    // ELOOP → io unconditionally (never reaches the invalid-asset stat check).
    assert_eq!(err.category(), "io");
}

#[test]
fn load_removed_referenced_png() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("guide.rollshot-guide");
    let commit = setup_project(&root);

    let sha = &commit.manifest.frames[0].sha256;
    let asset_path = root.join("assets/frames").join(format!("{sha}.png"));
    std::fs::remove_file(&asset_path).unwrap();

    let err = load_project(&root).unwrap_err();
    assert_eq!(err.category(), "io");
}

#[test]
fn create_rejects_pre_created_destination() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("guide.rollshot-guide");
    std::fs::create_dir_all(&root).unwrap();

    let pixels = pixel(640, 480);
    let snap = build_initial_snapshot(pixels.clone(), pixels);
    let err = create_project(&snap, &root).unwrap_err();
    assert_eq!(err.category(), "destination-exists");

    // Verify the pre-created dir is still empty (no partial writes)
    let entries: Vec<_> = std::fs::read_dir(&root)
        .unwrap()
        .filter_map(|e| e.ok())
        .collect();
    assert!(entries.is_empty(), "no partial writes on DestinationExists");
}

#[test]
fn save_project_as_rejects_pre_created_destination() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("guide.rollshot-guide");
    setup_project(&root);

    // Attempt Save As to the same existing destination
    let pixels = pixel(640, 480);
    let snap = build_initial_snapshot(pixels.clone(), pixels);
    let err = save_project_as(&snap, &root).unwrap_err();
    assert_eq!(err.category(), "destination-exists");

    // Original project untouched (still revision 1)
    let loaded = load_project(&root).unwrap();
    assert_eq!(loaded.manifest.revision, 1);
}

#[cfg(unix)]
#[test]
fn create_rejects_read_only_parent() {
    let dir = tempfile::tempdir().unwrap();
    let parent = dir.path().join("readonly-parent");
    std::fs::create_dir_all(&parent).unwrap();

    // Make parent read-only
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&parent, std::fs::Permissions::from_mode(0o555)).unwrap();

    let root = parent.join("guide.rollshot-guide");
    let pixels = pixel(640, 480);
    let snap = build_initial_snapshot(pixels.clone(), pixels);
    let err = create_project(&snap, &root).unwrap_err();
    assert_eq!(err.category(), "io");

    // Restore permissions for cleanup
    std::fs::set_permissions(&parent, std::fs::Permissions::from_mode(0o755)).unwrap();
}

#[test]
fn save_rejects_changed_base_revision() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("guide.rollshot-guide");
    let commit = setup_project(&root);

    // Externally bump revision
    let mut json = read_manifest_json(&root);
    json["revision"] = serde_json::Value::Number(99.into());
    std::fs::write(
        root.join("project.json"),
        serde_json::to_vec_pretty(&json).unwrap(),
    )
    .unwrap();

    let frame = &commit.manifest.frames[0];
    let snap = ProjectSnapshot {
        base_revision: Some(1),
        title: "E2E Guide".into(),
        capture_region: REGION,
        input_source: InputSourceKind::LinuxEvdev,
        input_capability: InputCapability::SemanticEvents,
        enabled_outputs: EnabledOutputs::default(),
        frames: vec![SnapshotFrame {
            id: 1,
            at_ms: 100,
            payload: SnapshotFramePayload::ExistingAsset {
                project_root: root.clone(),
                sha256: frame.sha256.clone(),
                width: frame.width,
                height: frame.height,
            },
        }],
        steps: vec![ProjectStep {
            id: ProjectStepId(1),
            order: 1,
            title: "Step".into(),
            caption: None,
            kind: CandidateKind::Click,
            reason: DetectReason::ClickConfirmed,
            at_ms: 150,
            keyframe: 1,
            nearby: vec![1],
            annotations: None,
        }],
    };

    let err = save_project(&snap, &root).unwrap_err();
    assert_eq!(err.category(), "revision-conflict");

    // Disk untouched — still revision 99
    let disk = load_project(&root).unwrap();
    assert_eq!(disk.manifest.revision, 99);
}

#[test]
fn load_rejects_unsupported_schema_version() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("guide.rollshot-guide");
    setup_project(&root);

    let mut json = read_manifest_json(&root);
    json["schema_version"] = serde_json::Value::Number(999.into());
    std::fs::write(
        root.join("project.json"),
        serde_json::to_vec_pretty(&json).unwrap(),
    )
    .unwrap();

    let err = load_project(&root).unwrap_err();
    assert_eq!(err.category(), "unsupported-schema");
}

#[test]
fn load_rejects_non_contiguous_step_order() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("guide.rollshot-guide");
    setup_project(&root);

    let mut json = read_manifest_json(&root);
    // Set step 1 order to 5 (non-contiguous)
    json["steps"][0]["order"] = serde_json::Value::Number(5.into());
    std::fs::write(
        root.join("project.json"),
        serde_json::to_vec_pretty(&json).unwrap(),
    )
    .unwrap();

    let err = load_project(&root).unwrap_err();
    assert_eq!(err.category(), "non-contiguous-order");
}
