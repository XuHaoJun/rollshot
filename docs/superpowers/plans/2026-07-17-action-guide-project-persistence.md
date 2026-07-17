# Action Guide Project Persistence Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add the versioned, strict, crash-safe `.rollshot-guide/` data contract, history-free annotation rehydration, content-addressed frame assets, and atomic create/load/save/Save As APIs required by later app work.

**Architecture:** `rollshot-image-document` owns serialization and rehydration of the current annotation graph only. `rollshot-action::project` owns v1 DTOs, structural validation, immutable PNG asset verification, and transactional filesystem persistence; it exposes project snapshots and commits without depending on iced or app state. A manifest is the commit point, frame assets are immutable, and an existing Save rejects a changed base revision.

**Tech Stack:** Rust 2021, serde/serde_json, image 0.25, sha2 0.10, rustix 1.1, tempfile, tracing, existing `rollshot-image-document` and `rollshot-action` crates.

## Global Constraints

- Authoritative spec: `docs/superpowers/specs/2026-07-17-action-guide-project-editing-design.md`.
- Project schema is exactly version 1; every project DTO uses `#[serde(deny_unknown_fields)]`.
- Persist current annotations only; never persist undo/redo, drafts, agent proposals, selection, modal, or other UI state.
- Preserve the exact editable Guide title, including an empty string; project validation must not apply the publish fallback.
- First committed revision is 1. Save As creates a new revision-1 project.
- Project frame filenames are derived as `assets/frames/<lowercase-sha256>.png`; paths are never serialized.
- The SHA-256 digest covers the encoded PNG bytes.
- Save-time validation fully decodes newly encoded PNG assets before commit.
- Open-time validation streams hashes and reads PNG dimensions without materializing RGBA pixels.
- Every tracing event uses an explicit stable `rollshot::*` target and structured fields; never log titles, captions, annotation text, image bytes, or title-bearing full paths.
- Writable network-filesystem behavior is outside v1 guarantees; this plan implements local-filesystem atomicity and revision conflict detection, while writer locking is Plan 2.
- No filesystem abstraction trait. Fault tests use real temporary directories and controlled filesystem damage.
- Commands in this plan are run from `/home/noah/rollshot` and must be prefixed with `rtk`.

---

## File Structure

**Create:**

- `crates/rollshot-action/src/project/mod.rs` — public project API and re-exports.
- `crates/rollshot-action/src/project/model.rs` — strict v1 manifest DTOs and in-memory save snapshot types.
- `crates/rollshot-action/src/project/error.rs` — privacy-safe typed project errors and categories.
- `crates/rollshot-action/src/project/validate.rs` — structural manifest/snapshot validation.
- `crates/rollshot-action/src/project/assets.rs` — deterministic PNG encoding, hashing, header inspection, and asset materialization.
- `crates/rollshot-action/src/project/store.rs` — load, first Save, Save As, existing Save, fsync, and atomic commit.
- `crates/rollshot-action/tests/project_persistence.rs` — public-API persistence, corruption, and transaction tests.

**Modify:**

- `Cargo.toml` — add `sha2 = "0.10"` as a workspace dependency.
- `crates/rollshot-action/Cargo.toml` — enable image-document serde and add sha2/rustix dependencies plus tempfile dev dependency.
- `crates/rollshot-image-document/Cargo.toml` — add serde_json for feature-gated round-trip tests.
- `crates/rollshot-action/src/lib.rs` — register and re-export the project API.
- `crates/rollshot-image-document/src/annotation.rs` — serde for `Annotation`.
- `crates/rollshot-image-document/src/document.rs` — validated history-free rehydration constructor.

---

### Task 1: Persist and rehydrate the committed annotation graph

**Files:**

- Modify: `crates/rollshot-image-document/Cargo.toml`
- Modify: `crates/rollshot-image-document/src/annotation.rs`
- Modify: `crates/rollshot-image-document/src/document.rs`
- Test: `crates/rollshot-image-document/src/document.rs`

**Interfaces:**

- Consumes: existing `Annotation`, `AnnotationId`, `ImageDocument`, geometry/style validators, and the optional crate feature `serde`.
- Produces:
  - `ImageDocument::validate_persisted_annotations(width: u32, height: u32, annotations: &[Annotation]) -> Result<(u64, u32), EditError>`
  - `ImageDocument::from_persisted_annotations(source: Arc<RgbaImage>, annotations: Vec<Annotation>) -> Result<ImageDocument, EditError>`.

- [ ] **Step 1: Write failing serde and rehydration tests**

Add tests that cover all annotation variants, stable IDs/callout numbers, empty history, and continued allocation:

```rust
#[cfg(feature = "serde")]
#[test]
fn persisted_annotations_round_trip_and_resume_allocators() {
    let source = Arc::new(RgbaImage::new(64, 64));
    let annotations = vec![
        Annotation::two_point(
            AnnotationId(4),
            TwoPointKind::Arrow,
            ImagePoint::new(1.0, 2.0),
            ImagePoint::new(20.0, 22.0),
        ),
        Annotation::number_callout(
            AnnotationId(9),
            7,
            ImagePoint::new(8.0, 8.0),
            ImagePoint::new(18.0, 18.0),
        ),
        Annotation::text_note(
            AnnotationId(12),
            ImagePoint::new(3.0, 4.0),
            "Open settings".into(),
        ),
        Annotation::opaque_redaction(
            AnnotationId(13),
            ImageRect::new(5.0, 5.0, 10.0, 10.0),
        ),
        Annotation::shape(
            AnnotationId(14),
            ShapeKind::Rectangle,
            ImageRect::new(2.0, 2.0, 8.0, 8.0),
        ),
        Annotation::freehand(
            AnnotationId(15),
            FreehandKind::Pen,
            vec![ImagePoint::new(1.0, 1.0), ImagePoint::new(4.0, 5.0)],
        ),
        Annotation::pixelate(
            AnnotationId(16),
            ImageRect::new(20.0, 20.0, 12.0, 12.0),
            DEFAULT_PIXELATE_BLOCK_SIZE,
        ),
    ];
    let json = serde_json::to_string(&annotations).unwrap();
    let restored: Vec<Annotation> = serde_json::from_str(&json).unwrap();
    let mut document = ImageDocument::from_persisted_annotations(source, restored).unwrap();

    assert_eq!(document.annotations(), annotations.as_slice());
    assert!(!document.can_undo());
    assert!(!document.can_redo());
    assert_eq!(document.state_id(), 0);

    let id = document.add_number_callout(
        ImagePoint::new(30.0, 30.0),
        ImagePoint::new(40.0, 40.0),
    );
    assert_eq!(id, AnnotationId(17));
    assert!(matches!(
        document.annotation(id),
        Some(Annotation::NumberCallout { number: 8, .. })
    ));
}

#[test]
fn persisted_annotations_reject_duplicate_ids() {
    let source = Arc::new(RgbaImage::new(32, 32));
    let annotations = vec![
        Annotation::opaque_redaction(AnnotationId(2), ImageRect::new(1.0, 1.0, 4.0, 4.0)),
        Annotation::pixelate(AnnotationId(2), ImageRect::new(8.0, 8.0, 4.0, 4.0), 8),
    ];
    assert!(matches!(
        ImageDocument::from_persisted_annotations(source, annotations),
        Err(EditError::DuplicateAnnotationId)
    ));
}
```

Add `serde_json = { workspace = true }` under `[dev-dependencies]` in `crates/rollshot-image-document/Cargo.toml` so the feature-gated test compiles.

- [ ] **Step 2: Run the focused tests and verify failure**

Run:

```bash
rtk cargo test -p rollshot-image-document --features serde persisted_annotations
```

Expected: compile failure because `Annotation` is not serializable and `from_persisted_annotations` / `DuplicateAnnotationId` do not exist.

- [ ] **Step 3: Add serde and the validating rehydration constructor**

On the existing `Annotation` declaration, keep its current derives and add these feature-gated attributes directly above the enum:

```rust
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
```

Extend `EditError` with stable persisted-data categories:

```rust
#[error("annotation ids must be non-zero")]
InvalidAnnotationId,
#[error("annotation ids must be unique")]
DuplicateAnnotationId,
#[error("number callout numbers must be non-zero")]
InvalidCalloutNumber,
#[error("annotation id or number allocation overflowed")]
AllocatorOverflow,
```

Implement the validator and constructor without replaying edit operations. The validator returns the next ID and callout number, so project structural validation can reuse it without allocating a bitmap:

```rust
pub fn validate_persisted_annotations(
    width: u32,
    height: u32,
    annotations: &[Annotation],
) -> Result<(u64, u32), EditError> {
    let mut ids = std::collections::BTreeSet::new();
    let mut max_id = 0u64;
    let mut max_number = 0u32;

    for annotation in &annotations {
        let id = annotation.id().0;
        if id == 0 {
            return Err(EditError::InvalidAnnotationId);
        }
        if !ids.insert(id) {
            return Err(EditError::DuplicateAnnotationId);
        }
        max_id = max_id.max(id);
        validate_persisted_annotation(annotation, width, height)?;
        if let Annotation::NumberCallout { number, .. } = annotation {
            if *number == 0 {
                return Err(EditError::InvalidCalloutNumber);
            }
            max_number = max_number.max(*number);
        }
    }

    let next_id = max_id.checked_add(1).ok_or(EditError::AllocatorOverflow)?;
    let next_number = max_number
        .checked_add(1)
        .ok_or(EditError::AllocatorOverflow)?;

    Ok((next_id.max(1), next_number.max(1)))
}

pub fn from_persisted_annotations(
    source: Arc<RgbaImage>,
    annotations: Vec<Annotation>,
) -> Result<Self, EditError> {
    let (width, height) = source.dimensions();
    let (next_id, next_number) =
        Self::validate_persisted_annotations(width, height, &annotations)?;

    Ok(Self {
        source,
        annotations,
        next_number,
        next_id,
        state_id: 0,
        next_state_id: 0,
        undo_stack: VecDeque::new(),
        redo_stack: Vec::new(),
    })
}
```

Add a private `validate_persisted_annotation` that matches every existing variant and calls the same finite/style/bounds checks used by edit operations. It must reject rather than clamp persisted coordinates. For `Pixelate`, reuse `MIN_PIXELATE_BLOCK_SIZE..=MAX_PIXELATE_BLOCK_SIZE`; for freehand require at least two distinct finite in-bounds points; for text require non-whitespace content; for every rectangle require finite positive bounds intersecting the source.

- [ ] **Step 4: Run the crate test suite**

Run:

```bash
rtk cargo test -p rollshot-image-document --features serde
```

Expected: all image-document tests pass, including all persisted annotation variants and allocation continuation.

- [ ] **Step 5: Commit**

```bash
rtk git add crates/rollshot-image-document/Cargo.toml crates/rollshot-image-document/src/annotation.rs crates/rollshot-image-document/src/document.rs
rtk git commit -m "feat(image-document): persist annotation snapshots"
```

---

### Task 2: Define strict project v1 DTOs and save snapshots

**Files:**

- Modify: `Cargo.toml`
- Modify: `crates/rollshot-action/Cargo.toml`
- Create: `crates/rollshot-action/src/project/mod.rs`
- Create: `crates/rollshot-action/src/project/model.rs`
- Create: `crates/rollshot-action/src/project/error.rs`
- Modify: `crates/rollshot-action/src/lib.rs`
- Test: `crates/rollshot-action/src/project/model.rs`

**Interfaces:**

- Consumes: Task 1 serializable `Annotation`; existing `CaptureRegion`, `InputSourceKind`, `InputCapability`, `CandidateKind`, `DetectReason`, `FrameId`, and `Millis`.
- Produces: `ProjectManifestV1`, `ProjectStepId`, `ProjectFrame`, `ProjectStep`, `PersistedStepAnnotations`, `EnabledOutputs`, `ProjectSnapshot`, `SnapshotFrame`, `SnapshotFramePayload`, `LoadedProject`, `ProjectCommit`, and `ProjectError`.

- [ ] **Step 1: Write failing strict-schema tests**

Create `project/model.rs` tests that deserialize a minimal valid fixture and reject an unknown field:

```rust
#[test]
fn manifest_rejects_unknown_fields() {
    let json = serde_json::json!({
        "schema_version": 1,
        "revision": 1,
        "title": "Guide",
        "capture_region": { "x": 0, "y": 0, "width": 8, "height": 8 },
        "input_source": "visual-only",
        "input_capability": { "visual-only": { "reason": "source-start-failed" } },
        "enabled_outputs": { "storyboard": false, "gif": false, "mp4": false },
        "frames": [],
        "steps": [],
        "surprise": true
    });
    let error = serde_json::from_value::<ProjectManifestV1>(json).unwrap_err();
    assert!(error.to_string().contains("unknown field"));
}
```

- [ ] **Step 2: Run the focused test and verify failure**

Run:

```bash
rtk cargo test -p rollshot-action project::model::tests::manifest_rejects_unknown_fields
```

Expected: compile failure because the project module and DTO do not exist.

- [ ] **Step 3: Add dependencies and exact project types**

Add `sha2 = "0.10"` to workspace dependencies. In `rollshot-action` enable `rollshot-image-document = { path = "../rollshot-image-document", features = ["serde"] }`, add `sha2 = { workspace = true }`, `rustix = { workspace = true }`, and `tempfile = "3"` under dev dependencies.

Define the following public model shape; every serialized struct gets `deny_unknown_fields` and snake_case field names remain the serde default:

```rust
pub const PROJECT_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ProjectStepId(pub u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct EnabledOutputs {
    pub storyboard: bool,
    pub gif: bool,
    pub mp4: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectFrame {
    pub id: FrameId,
    pub at_ms: Millis,
    pub sha256: String,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PersistedStepAnnotations {
    pub annotations: Vec<rollshot_image_document::Annotation>,
    pub explanations: std::collections::BTreeMap<rollshot_image_document::AnnotationId, String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectStep {
    pub id: ProjectStepId,
    pub order: usize,
    pub title: String,
    pub caption: Option<String>,
    pub kind: CandidateKind,
    pub reason: DetectReason,
    pub at_ms: Millis,
    pub keyframe: FrameId,
    pub nearby: Vec<FrameId>,
    pub annotations: Option<PersistedStepAnnotations>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectManifestV1 {
    pub schema_version: u32,
    pub revision: u64,
    pub title: String,
    pub capture_region: CaptureRegion,
    pub input_source: InputSourceKind,
    pub input_capability: InputCapability,
    pub enabled_outputs: EnabledOutputs,
    pub frames: Vec<ProjectFrame>,
    pub steps: Vec<ProjectStep>,
}
```

Keep filesystem/runtime snapshot types non-serializable:

```rust
#[derive(Clone)]
pub enum SnapshotFramePayload {
    Pixels(std::sync::Arc<image::RgbaImage>),
    ExistingAsset {
        project_root: std::path::PathBuf,
        sha256: String,
        width: u32,
        height: u32,
    },
}

#[derive(Clone)]
pub struct SnapshotFrame {
    pub id: FrameId,
    pub at_ms: Millis,
    pub payload: SnapshotFramePayload,
}

#[derive(Clone)]
pub struct ProjectSnapshot {
    pub base_revision: Option<u64>,
    pub title: String,
    pub capture_region: CaptureRegion,
    pub input_source: InputSourceKind,
    pub input_capability: InputCapability,
    pub enabled_outputs: EnabledOutputs,
    pub frames: Vec<SnapshotFrame>,
    pub steps: Vec<ProjectStep>,
}

#[derive(Debug, Clone)]
pub struct LoadedProject {
    pub root: std::path::PathBuf,
    pub manifest: ProjectManifestV1,
}

#[derive(Debug, Clone)]
pub struct ProjectCommit {
    pub root: std::path::PathBuf,
    pub manifest: ProjectManifestV1,
}
```

Define `ProjectError` variants with privacy-safe messages and a `category()` method: `Io`, `InvalidJson`, `UnsupportedSchema`, `InvalidManifest { category, step_id, frame_id }`, `InvalidAsset { category, frame_id }`, `Encode`, `DestinationExists`, `UnsupportedAtomicCommit`, and `RevisionConflict { expected, actual }`. Paths may be used in user-facing `Display`, but tracing call sites must log only `category()` and structural IDs.

- [ ] **Step 4: Register the module and re-exports**

In `lib.rs`, add `pub mod project;` and re-export only through that namespace; do not flatten the full project API into the crate root.

- [ ] **Step 5: Run focused tests**

Run:

```bash
rtk cargo test -p rollshot-action project::model
```

Expected: strict-schema and model serde tests pass.

- [ ] **Step 6: Commit**

```bash
rtk git add Cargo.toml Cargo.lock crates/rollshot-action/Cargo.toml crates/rollshot-action/src/lib.rs crates/rollshot-action/src/project
rtk git commit -m "feat(action): define project v1 schema"
```

---

### Task 3: Validate project structure and annotation ownership

**Files:**

- Create: `crates/rollshot-action/src/project/validate.rs`
- Modify: `crates/rollshot-action/src/project/mod.rs`
- Test: `crates/rollshot-action/src/project/validate.rs`

**Interfaces:**

- Consumes: Task 2 `ProjectManifestV1` and `ProjectSnapshot`.
- Produces: `validate_manifest_structure(&ProjectManifestV1) -> Result<(), ProjectError>` and `validate_snapshot_structure(&ProjectSnapshot) -> Result<(), ProjectError>`.

- [ ] **Step 1: Write the failing validation matrix**

Use a one-step 8×8 fixture and add separate tests for: schema != 1, revision 0, no steps, duplicate frame/step IDs, non-contiguous order, missing keyframe, current keyframe absent from nearby, duplicate nearby IDs, frame dimensions differing from capture region, annotation explanation referring to a missing annotation, annotation document on a step whose keyframe is missing, and final-step deletion represented as an empty snapshot.

One representative test:

```rust
#[test]
fn keyframe_must_be_present_in_nearby() {
    let mut manifest = valid_manifest();
    manifest.steps[0].keyframe = 9;
    manifest.steps[0].nearby = vec![7, 8];
    let error = validate_manifest_structure(&manifest).unwrap_err();
    assert_eq!(error.category(), "keyframe_not_nearby");
}
```

- [ ] **Step 2: Run tests and verify failure**

Run:

```bash
rtk cargo test -p rollshot-action project::validate
```

Expected: compile failure because validation functions do not exist.

- [ ] **Step 3: Implement structural validation**

Validation must run in deterministic order so tests receive stable categories:

```rust
pub fn validate_manifest_structure(manifest: &ProjectManifestV1) -> Result<(), ProjectError> {
    if manifest.schema_version != PROJECT_SCHEMA_VERSION {
        return Err(ProjectError::UnsupportedSchema {
            version: manifest.schema_version,
        });
    }
    if manifest.revision == 0 {
        return Err(ProjectError::invalid_manifest("zero_revision", None, None));
    }
    validate_common(
        manifest.capture_region,
        &manifest.frames,
        &manifest.steps,
    )
}

pub fn validate_snapshot_structure(snapshot: &ProjectSnapshot) -> Result<(), ProjectError> {
    let frames = snapshot.frames.iter().map(ProjectFrame::from_snapshot_metadata).collect::<Vec<_>>();
    validate_common(snapshot.capture_region, &frames, &snapshot.steps)
}
```

`validate_common` must use `BTreeSet`s for IDs, require exactly `order == offset + 1`, require non-zero unique step IDs, require at least one frame and one step, require every referenced frame and explanation target to exist, require every frame dimensions to equal non-zero capture-region dimensions, and require each optional annotation list to pass `ImageDocument::validate_persisted_annotations` with the referenced frame dimensions. It accepts an empty Guide title unchanged. Structural validation must not allocate an RGBA bitmap.

- [ ] **Step 4: Run validation tests**

Run:

```bash
rtk cargo test -p rollshot-action project::validate
```

Expected: the full validation matrix passes with stable categories.

- [ ] **Step 5: Commit**

```bash
rtk git add crates/rollshot-action/src/project/mod.rs crates/rollshot-action/src/project/validate.rs
rtk git commit -m "feat(action): validate project manifests"
```

---

### Task 4: Encode and verify content-addressed frame assets

**Files:**

- Create: `crates/rollshot-action/src/project/assets.rs`
- Modify: `crates/rollshot-action/src/project/mod.rs`
- Test: `crates/rollshot-action/src/project/assets.rs`

**Interfaces:**

- Consumes: Task 2 snapshot frame payloads and `ProjectError`.
- Produces: `encode_png_asset`, `inspect_png_asset`, `decode_png_asset`, `materialize_asset`, `asset_relative_path`, and `InspectedAsset`.

- [ ] **Step 1: Write failing asset tests**

Cover deterministic digest/path, byte deduplication, header-only inspection, hash mismatch, corrupt header, a symlinked `assets` directory, a symlinked `frames` directory, a symlinked PNG, replacement between validation stages, digest-valid invalid PNG pixel data, full decode verification for newly encoded assets, and copying an existing asset during Save As.

```rust
#[test]
fn encoded_asset_digest_drives_derived_path() {
    let image = RgbaImage::from_pixel(4, 3, Rgba([1, 2, 3, 255]));
    let encoded = encode_png_asset(&image).unwrap();
    assert_eq!(encoded.sha256.len(), 64);
    assert!(encoded.sha256.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase()));
    assert_eq!(
        asset_relative_path(&encoded.sha256),
        PathBuf::from(format!("assets/frames/{}.png", encoded.sha256))
    );
    image::load_from_memory_with_format(&encoded.bytes, image::ImageFormat::Png).unwrap();
}
```

- [ ] **Step 2: Run tests and verify failure**

Run:

```bash
rtk cargo test -p rollshot-action project::assets
```

Expected: compile failure because asset helpers do not exist.

- [ ] **Step 3: Implement deterministic encoding and inspection**

Define:

```rust
pub(crate) struct EncodedAsset {
    pub bytes: Vec<u8>,
    pub sha256: String,
    pub width: u32,
    pub height: u32,
}

pub(crate) struct InspectedAsset {
    pub sha256: String,
    pub width: u32,
    pub height: u32,
}
```

Use `image::codecs::png::PngEncoder` and `ImageEncoder::write_image` to encode RGBA8 bytes, then hash the exact encoded buffer with `Sha256`. Add one Unix project-asset opener that opens the project root, then `assets`, then `frames`, then the derived digest filename through relative directory handles using `rustix` `openat` with `NOFOLLOW` (and `DIRECTORY` for directory components), verifies the final handle is a regular file with `fstat`, and returns the same owned handle used for all subsequent reads. No validation stage may reopen by a path that could be swapped.

`inspect_png_asset(root, frame)` streams that handle through `Sha256`, seeks the same handle, calls `ImageReader::with_guessed_format()?.into_dimensions()`, and compares digest/dimensions without calling `decode()`. `decode_png_asset(root, frame)` performs the same hash/header checks and then fully decodes from that verified handle; Plan 2 uses it for lazy resolution. `materialize_asset` must:

- encode and fully decode `Pixels` before writing;
- for `ExistingAsset`, stream-verify its digest/header and copy bytes without RGBA decode;
- write a unique temp sibling under `assets/frames/`, fsync it, and rename only if the final digest path does not already exist;
- if the final path exists, verify it and discard the temp file;
- return `ProjectFrame` metadata.

- [ ] **Step 4: Run asset tests**

Run:

```bash
rtk cargo test -p rollshot-action project::assets
```

Expected: all content-addressing and corruption tests pass.

- [ ] **Step 5: Commit**

```bash
rtk git add crates/rollshot-action/src/project/assets.rs crates/rollshot-action/src/project/mod.rs
rtk git commit -m "feat(action): store content-addressed project frames"
```

---

### Task 5: Implement atomic first Save, Save As, load, and existing Save

**Files:**

- Create: `crates/rollshot-action/src/project/store.rs`
- Modify: `crates/rollshot-action/src/project/mod.rs`
- Test: `crates/rollshot-action/src/project/store.rs`

**Interfaces:**

- Consumes: Tasks 2–4 snapshot/model/asset/validation APIs.
- Produces:
  - `create_project(snapshot: &ProjectSnapshot, destination: &Path) -> Result<ProjectCommit, ProjectError>`
  - `save_project(snapshot: &ProjectSnapshot, project_root: &Path) -> Result<ProjectCommit, ProjectError>`
  - `save_project_as(snapshot: &ProjectSnapshot, destination: &Path) -> Result<ProjectCommit, ProjectError>`
  - `load_project(project_root: &Path) -> Result<LoadedProject, ProjectError>`

- [ ] **Step 1: Write failing transaction tests**

Tests must prove:

- first Save creates revision 1 and never replaces an existing destination;
- Save As resets revision to 1 and copies existing assets;
- existing Save increments once;
- a changed on-disk revision returns `RevisionConflict` and does not overwrite;
- missing/hash-mismatched/header-invalid or symlink-escaped assets fail load;
- unknown fields and newer schemas return stable errors;
- a pre-created commit destination leaves no final partial project;
- temporary directories/files are cleaned after failure.

Representative conflict test:

```rust
#[test]
fn existing_save_rejects_external_revision_change() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("guide.rollshot-guide");
    let first = create_project(&snapshot(None), &root).unwrap();
    let mut external = first.manifest.clone();
    external.revision = 2;
    std::fs::write(
        root.join("project.json"),
        serde_json::to_vec_pretty(&external).unwrap(),
    ).unwrap();

    let error = save_project(&snapshot(Some(1)), &root).unwrap_err();
    assert!(matches!(
        error,
        ProjectError::RevisionConflict { expected: 1, actual: 2 }
    ));
    let disk: ProjectManifestV1 = serde_json::from_slice(
        &std::fs::read(root.join("project.json")).unwrap(),
    ).unwrap();
    assert_eq!(disk.revision, 2);
}
```

- [ ] **Step 2: Run tests and verify failure**

Run:

```bash
rtk cargo test -p rollshot-action project::store
```

Expected: compile failure because store functions do not exist.

- [ ] **Step 3: Implement load and manifest durability helpers**

Add helpers with these exact responsibilities:

```rust
fn read_manifest(root: &Path) -> Result<ProjectManifestV1, ProjectError>;
fn write_manifest_atomic(root: &Path, manifest: &ProjectManifestV1) -> Result<(), ProjectError>;
fn fsync_dir(path: &Path) -> Result<(), ProjectError>;
fn commit_noreplace(temp: &Path, destination: &Path) -> Result<(), ProjectError>;
```

`read_manifest` parses strict JSON and validates structure, then verifies every referenced asset with `inspect_png_asset`; it does not decode RGBA. `write_manifest_atomic` writes `project.json.tmp-<pid>-<counter>`, calls `sync_all`, same-directory renames to `project.json`, then opens and syncs the project directory. `commit_noreplace` uses `rustix::fs::renameat_with(..., RenameFlags::NOREPLACE)` and maps `EXIST`, `NOSYS`, `INVAL`, and `NOTSUP` separately.

- [ ] **Step 4: Implement new-project and Save As transactions**

Both functions build a unique temp sibling guarded by an RAII cleanup type, materialize every snapshot frame, validate the resulting revision-1 manifest, write it durably, create empty `publish/`, then no-replace rename the temp directory. `create_project` rejects `snapshot.base_revision.is_some()`; `save_project_as` accepts either base state but always writes revision 1.

- [ ] **Step 5: Implement existing Save with revision preflight**

`save_project` requires `snapshot.base_revision == Some(expected)`. Before writing assets, load the current manifest and compare revision. Materialize missing immutable assets, then re-read revision immediately before manifest commit and reject if it changed. Build revision `expected + 1` with checked arithmetic. A conflict leaves local snapshot ownership with the caller and the external manifest untouched.

- [ ] **Step 6: Run store tests**

Run:

```bash
rtk cargo test -p rollshot-action project::store
```

Expected: transaction, collision, load-corruption, and revision-conflict tests pass.

- [ ] **Step 7: Commit**

```bash
rtk git add crates/rollshot-action/src/project/store.rs crates/rollshot-action/src/project/mod.rs
rtk git commit -m "feat(action): persist projects atomically"
```

---

### Task 6: Prove the public persistence contract end to end

**Files:**

- Create: `crates/rollshot-action/tests/project_persistence.rs`
- Modify: `crates/rollshot-action/src/project/mod.rs`

**Interfaces:**

- Consumes: all Plan 1 public APIs.
- Produces: a stable public contract ready for Plan 2 app adapters.

- [ ] **Step 1: Add a public-API round-trip fixture**

Build a two-step project with shared/duplicate frame pixels, one changed keyframe, all annotation variants, one explanation, and Storyboard/MP4 enabled. Create → close values → load → build a Save snapshot using `ExistingAsset` payloads → Save → load again. Assert:

- revisions are 1 then 2;
- shared PNG bytes produce one asset file;
- exact titles/captions/order/semantic metadata survive;
- keyframe and nearby order survive;
- annotations and explanation IDs survive;
- no undo/redo state is serialized anywhere in `project.json`;
- all serialized frame paths are derived rather than present as fields.

- [ ] **Step 2: Add real-filesystem damage tests**

In separate tempdirs: truncate `project.json`, inject an unknown field, mutate one PNG byte, replace one PNG with invalid header bytes, replace `assets`, `frames`, or one PNG with a symlink to valid external content, remove a referenced PNG, pre-create the destination, and chmod a writable temp path read-only on Unix. Each test asserts a stable `ProjectError::category()` and that the last committed manifest remains readable where applicable.

- [ ] **Step 3: Run Plan 1 verification**

Run:

```bash
rtk cargo test -p rollshot-image-document --features serde
rtk cargo test -p rollshot-action
rtk cargo fmt --check
rtk cargo clippy -p rollshot-image-document -p rollshot-action --all-targets -- -D warnings
```

Expected: all commands exit 0 with no warnings.

- [ ] **Step 4: Commit**

```bash
rtk git add crates/rollshot-action/tests/project_persistence.rs crates/rollshot-action/src/project/mod.rs
rtk git commit -m "test(action): cover project persistence contract"
```

## Plan 1 Completion Gate

Before starting Plan 2, verify all of the following from a clean checkout:

- `ProjectManifestV1` is strict and versioned.
- Annotation rehydration preserves IDs/numbers and starts empty history.
- First Save and Save As never replace an existing directory.
- Existing Save is durable and rejects a changed base revision.
- Open validates all asset digests/headers without RGBA decode.
- Content-addressed assets deduplicate and survive Save As.
- `rtk cargo test -p rollshot-action` and the image-document serde lane pass.
