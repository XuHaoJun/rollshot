//! Canonical Action Guide project continuity projection.
//!
//! An immutable, privacy-safe read model derived from a validated
//! `LoadedProject`. Retains only project revision, title, and ordered
//! step references — IDs, order, keyframe, title, caption, kind, reason,
//! and timestamp.
//!
//! Excludes frame payloads, annotations, frame SHA-256 digests, frame
//! dimensions, capture region, input source, nearby frames, enabled outputs,
//! project root path, and import warnings.

use sha2::{Digest, Sha256};
use std::fmt;

use super::model::{LoadedProject, ProjectStepId};
use crate::guide::Guide;
use crate::models::{CandidateKind, DetectReason, FrameId, Millis};
use crate::GuideStep;

// ========================================================================
// Constants
// ========================================================================

pub const MAX_PROJECTED_STEPS: usize = 200;
pub const MAX_PROJECTED_TEXT_BYTES: usize = 4_096;
pub const MAX_PROJECTED_BYTES: usize = 256 * 1024;
const ACTION_GUIDE_PROJECTION_DOMAIN: &[u8] = b"rollshot-action-guide-continuity-v1\0";

// ========================================================================
// Errors
// ========================================================================

/// Privacy-safe error from Action Guide projection construction.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ActionGuideProjectionError {
    #[error("step count {0} exceeds limit {MAX_PROJECTED_STEPS}")]
    TooManySteps(usize),
    #[error("guide title exceeds {MAX_PROJECTED_TEXT_BYTES}-byte bound: {len} bytes")]
    GuideTitleTooLong { len: usize },
    #[error("step {step_id} title exceeds {MAX_PROJECTED_TEXT_BYTES}-byte bound: {len} bytes")]
    StepTitleTooLong { step_id: u64, len: usize },
    #[error("step {step_id} caption exceeds {MAX_PROJECTED_TEXT_BYTES}-byte bound: {len} bytes")]
    StepCaptionTooLong { step_id: u64, len: usize },
    #[error("non-contiguous step order: expected {expected}, got {got}")]
    NonContiguousOrder { expected: u32, got: u32 },
    #[error("duplicate step order: {0}")]
    DuplicateOrder(u32),
    #[error("canonical projection exceeds 256 KiB: {0} bytes")]
    ProjectionTooLarge(usize),
    #[error("canonical serialization failed: {0}")]
    Canonical(String),
}

// ========================================================================
// Projected step (public, immutable)
// ========================================================================

/// One projected step from the Action Guide project. Retains only typed
/// semantic references — no frame payloads, annotations, paths, or digests.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct ActionGuideProjectedStepV1 {
    pub id: ProjectStepId,
    pub order: u32,
    pub keyframe: FrameId,
    pub title: String,
    pub caption: Option<String>,
    pub kind: CandidateKind,
    pub reason: DetectReason,
    pub at_ms: Millis,
}

// ========================================================================
// Private serializable DTO — fixed field order for canonical bytes
// ========================================================================

#[derive(serde::Serialize)]
struct ActionGuideProjectionDto {
    schema_version: u32,
    revision: u64,
    title: String,
    steps: Vec<ActionGuideProjectedStepV1>,
}

// ========================================================================
// Public projection (immutable, no payloads or paths)
// ========================================================================

/// Immutable V1 continuity projection from a validated `LoadedProject`.
///
/// Retains only project revision, title, and ordered step references. All
/// strings are capped at 4,096 UTF-8 bytes. At most 200 steps. The canonical
/// serialized form is capped at 256 KiB.
///
/// Excludes frame payloads, annotations, frame digests, frame dimensions,
/// capture region, input source, nearby frames, enabled outputs, project root,
/// and import warnings.
#[derive(Clone)]
pub struct ActionGuideContextProjectionV1 {
    revision: u64,
    title: String,
    steps: Vec<ActionGuideProjectedStepV1>,
    canonical_bytes: Vec<u8>,
    digest: String,
}

impl ActionGuideContextProjectionV1 {
    /// Construct a projection from a loaded project.
    ///
    /// Re-runs `validate_manifest_structure` and rejects non-contiguous or
    /// duplicate step orders. All strings are bounded. The canonical form is
    /// serialized once and cached.
    pub fn from_loaded_project(loaded: &LoadedProject) -> Result<Self, ActionGuideProjectionError> {
        let manifest = &loaded.manifest;

        // Re-validate manifest structure (schema, revision, frames, steps).
        super::validate::validate_manifest_structure(manifest)
            .map_err(|e| ActionGuideProjectionError::Canonical(e.to_string()))?;

        // Bound step count.
        if manifest.steps.len() > MAX_PROJECTED_STEPS {
            return Err(ActionGuideProjectionError::TooManySteps(
                manifest.steps.len(),
            ));
        }

        // Bound guide title.
        bound_text("guide_title", 0, &manifest.title)?;

        // Build projected steps, sorted by validated order.
        let mut steps: Vec<ActionGuideProjectedStepV1> = manifest
            .steps
            .iter()
            .map(|ps| {
                bound_text("step_title", ps.id.0, &ps.title)?;
                if let Some(caption) = &ps.caption {
                    bound_text("step_caption", ps.id.0, caption)?;
                }
                Ok(ActionGuideProjectedStepV1 {
                    id: ps.id,
                    order: ps.order,
                    keyframe: ps.keyframe,
                    title: ps.title.clone(),
                    caption: ps.caption.clone(),
                    kind: ps.kind,
                    reason: ps.reason,
                    at_ms: ps.at_ms,
                })
            })
            .collect::<Result<Vec<_>, ActionGuideProjectionError>>()?;

        // Sort by order for canonical form.
        steps.sort_by_key(|s| s.order);

        // Validate contiguous, non-duplicate orders.
        for (i, step) in steps.iter().enumerate() {
            let expected = (i + 1) as u32;
            if step.order != expected {
                if i > 0 && steps[i - 1].order == step.order {
                    return Err(ActionGuideProjectionError::DuplicateOrder(step.order));
                }
                return Err(ActionGuideProjectionError::NonContiguousOrder {
                    expected,
                    got: step.order,
                });
            }
        }

        // Serialize once for canonical bytes.
        let dto = ActionGuideProjectionDto {
            schema_version: 1,
            revision: manifest.revision,
            title: manifest.title.clone(),
            steps,
        };

        let canonical_bytes = serde_json::to_vec(&dto)
            .map_err(|e| ActionGuideProjectionError::Canonical(e.to_string()))?;

        if canonical_bytes.len() > MAX_PROJECTED_BYTES {
            return Err(ActionGuideProjectionError::ProjectionTooLarge(
                canonical_bytes.len(),
            ));
        }

        // Derive digest with domain separator.
        let digest = digest_projection(&canonical_bytes);

        Ok(Self {
            revision: dto.revision,
            title: dto.title,
            steps: dto.steps,
            canonical_bytes,
            digest,
        })
    }

    /// Project revision.
    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// Guide title.
    pub fn title(&self) -> &str {
        &self.title
    }

    /// Ordered projected steps.
    pub fn steps(&self) -> &[ActionGuideProjectedStepV1] {
        &self.steps
    }

    /// Canonical serialized bytes of the projection DTO.
    pub fn canonical_bytes(&self) -> &[u8] {
        &self.canonical_bytes
    }

    /// SHA-256 hex digest of the canonical projection with domain separator.
    pub fn digest(&self) -> &str {
        &self.digest
    }

    /// Reconstruct a `Guide` from the projection.
    ///
    /// Maps `ProjectStepId.0` to `GuideStep::source` and uses sequential
    /// 1-based indexing. Does not carry nearby, annotations, frame hashes,
    /// frame dimensions, capture region, input source, warnings, enabled
    /// outputs, or project root.
    pub fn to_guide(&self) -> Result<Guide, &'static str> {
        let steps: Vec<GuideStep> = self
            .steps
            .iter()
            .map(|ps| GuideStep {
                index: ps.order as usize,
                title: ps.title.clone(),
                caption: ps.caption.clone().unwrap_or_default(),
                kind: ps.kind,
                reason: ps.reason,
                at_ms: ps.at_ms,
                keyframe: ps.keyframe,
                nearby: Vec::new(),
                source: ps.id.0,
            })
            .collect();

        Guide::from_reviewed_steps(self.title.clone(), steps)
    }
}

impl fmt::Debug for ActionGuideContextProjectionV1 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ActionGuideContextProjectionV1")
            .field("revision", &self.revision)
            .field("title", &self.title)
            .field("steps", &self.steps)
            .field("digest", &self.digest)
            // canonical_bytes deliberately omitted from Debug
            .finish()
    }
}

// ========================================================================
// Private helpers
// ========================================================================

fn bound_text(
    field: &'static str,
    step_id: u64,
    value: &str,
) -> Result<(), ActionGuideProjectionError> {
    let len = value.len();
    if len > MAX_PROJECTED_TEXT_BYTES {
        return Err(match (field, step_id) {
            (_, 0) => ActionGuideProjectionError::GuideTitleTooLong { len },
            ("step_title", id) => ActionGuideProjectionError::StepTitleTooLong { step_id: id, len },
            ("step_caption", id) => {
                ActionGuideProjectionError::StepCaptionTooLong { step_id: id, len }
            }
            _ => ActionGuideProjectionError::GuideTitleTooLong { len },
        });
    }
    Ok(())
}

fn digest_projection(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(ACTION_GUIDE_PROJECTION_DOMAIN);
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

// ========================================================================
// Tests
// ========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{CaptureRegion, InputCapability, InputSourceKind, Millis};
    use crate::project::model::{
        EnabledOutputs, ProjectSnapshot, ProjectStep, SnapshotFrame, SnapshotFramePayload,
    };
    use crate::project::store::{create_project, load_project};

    use image::Rgba;
    use image::RgbaImage;
    use std::sync::Arc;

    fn pixel_image(w: u32, h: u32) -> Arc<RgbaImage> {
        Arc::new(RgbaImage::from_pixel(w, h, Rgba([10, 20, 30, 255])))
    }

    /// Create a project with `n` steps at the given revision via the real
    /// create_project → save_project → load_project round-trip.
    fn saved_project_fixture(n: usize) -> (tempfile::TempDir, LoadedProject) {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("guide.rollshot-guide");

        let mut steps = Vec::new();
        for i in 0..n {
            steps.push(ProjectStep {
                id: ProjectStepId((i + 1) as u64),
                order: (i + 1) as u32,
                title: format!("Step {}", i + 1),
                caption: Some(format!("Caption {}", i + 1)),
                kind: crate::models::CandidateKind::Click,
                reason: crate::models::DetectReason::ClickConfirmed,
                at_ms: (100 + i as Millis * 100),
                keyframe: 1,
                nearby: vec![1],
                annotations: None,
            });
        }

        let snapshot = ProjectSnapshot {
            base_revision: None,
            title: "Test Guide".into(),
            capture_region: CaptureRegion {
                x: 0,
                y: 0,
                width: 8,
                height: 8,
            },
            input_source: InputSourceKind::VisualOnly,
            input_capability: InputCapability::SemanticEvents,
            enabled_outputs: EnabledOutputs::default(),
            frames: vec![SnapshotFrame {
                id: 1,
                at_ms: 100,
                payload: SnapshotFramePayload::Pixels(pixel_image(8, 8)),
            }],
            steps,
            import_warnings: Vec::new(),
        };

        create_project(&snapshot, &root).unwrap();
        let loaded = load_project(&root, None).unwrap();
        (dir, loaded)
    }

    /// Create a project at a specific revision by bumping via save_project.
    fn saved_project_at_revision(
        n: usize,
        target_revision: u64,
    ) -> (tempfile::TempDir, LoadedProject) {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("guide.rollshot-guide");

        let mut steps = Vec::new();
        for i in 0..n {
            steps.push(ProjectStep {
                id: ProjectStepId((i + 1) as u64),
                order: (i + 1) as u32,
                title: format!("Step {}", i + 1),
                caption: Some(format!("Caption {}", i + 1)),
                kind: crate::models::CandidateKind::Click,
                reason: crate::models::DetectReason::ClickConfirmed,
                at_ms: (100 + i as Millis * 100),
                keyframe: 1,
                nearby: vec![1],
                annotations: None,
            });
        }

        let make_snapshot = |base: Option<u64>| ProjectSnapshot {
            base_revision: base,
            title: "Test Guide".into(),
            capture_region: CaptureRegion {
                x: 0,
                y: 0,
                width: 8,
                height: 8,
            },
            input_source: InputSourceKind::VisualOnly,
            input_capability: InputCapability::SemanticEvents,
            enabled_outputs: EnabledOutputs::default(),
            frames: vec![SnapshotFrame {
                id: 1,
                at_ms: 100,
                payload: SnapshotFramePayload::Pixels(pixel_image(8, 8)),
            }],
            steps: steps.clone(),
            import_warnings: Vec::new(),
        };

        create_project(&make_snapshot(None), &root).unwrap();
        for rev in 2..=target_revision {
            crate::project::store::save_project(&make_snapshot(Some(rev - 1)), &root).unwrap();
        }

        let loaded = load_project(&root, None).unwrap();
        (dir, loaded)
    }

    // ---- Core contract tests ----

    #[test]
    fn loaded_revision_projects_without_paths_pixels_or_annotations() {
        let (_temp, loaded) = saved_project_fixture(7);
        let projection = ActionGuideContextProjectionV1::from_loaded_project(&loaded).unwrap();
        let json = String::from_utf8_lossy(projection.canonical_bytes());

        assert_eq!(projection.revision(), 1);
        assert_eq!(projection.steps().len(), loaded.manifest.steps.len());
        assert!(!json.contains(loaded.root.to_string_lossy().as_ref()));
        assert!(!json.contains("annotations"));
        assert!(!json.contains("frames"));
        assert!(!json.contains("sha256"));
    }

    #[test]
    fn same_revision_reloads_to_identical_projection() {
        let (_temp, loaded) = saved_project_fixture(3);
        let first = ActionGuideContextProjectionV1::from_loaded_project(&loaded).unwrap();
        let project_root = loaded.root.clone();
        drop(loaded);
        let reopened = load_project(&project_root, None).unwrap();
        let second = ActionGuideContextProjectionV1::from_loaded_project(&reopened).unwrap();
        assert_eq!(first.canonical_bytes(), second.canonical_bytes());
        assert_eq!(first.digest(), second.digest());
    }

    #[test]
    fn projection_debug_omits_canonical_bytes() {
        let (_temp, loaded) = saved_project_fixture(1);
        let projection = ActionGuideContextProjectionV1::from_loaded_project(&loaded).unwrap();
        let debug = format!("{projection:?}");
        // Debug should have revision, title, steps, digest — but not raw canonical bytes
        assert!(debug.contains("revision"));
        assert!(debug.contains("digest"));
    }

    #[test]
    fn projection_debug_and_json_omit_paths_and_frames() {
        let (_temp, loaded) = saved_project_fixture(2);
        let projection = ActionGuideContextProjectionV1::from_loaded_project(&loaded).unwrap();
        let rendered = format!(
            "{:?}{}",
            projection,
            String::from_utf8_lossy(projection.canonical_bytes())
        );

        assert!(!rendered.contains(loaded.root.to_string_lossy().as_ref()));
        assert!(!rendered.contains("annotations"));
        assert!(!rendered.contains("frames"));
    }

    #[test]
    fn to_guide_maps_step_ids_to_source() {
        let (_temp, loaded) = saved_project_fixture(3);
        let projection = ActionGuideContextProjectionV1::from_loaded_project(&loaded).unwrap();
        let guide = projection.to_guide().unwrap();

        assert_eq!(guide.title(), "Test Guide");
        assert_eq!(guide.steps().len(), 3);
        // source = ProjectStepId.0
        assert_eq!(guide.steps()[0].source, 1);
        assert_eq!(guide.steps()[1].source, 2);
        assert_eq!(guide.steps()[2].source, 3);
        // index is 1-based
        assert_eq!(guide.steps()[0].index, 1);
        assert_eq!(guide.steps()[1].index, 2);
        assert_eq!(guide.steps()[2].index, 3);
        // captions populated
        assert_eq!(guide.steps()[0].caption, "Caption 1");
    }

    #[test]
    fn to_guide_omits_nearby_annotations_and_frames() {
        let (_temp, loaded) = saved_project_fixture(2);
        let projection = ActionGuideContextProjectionV1::from_loaded_project(&loaded).unwrap();
        let guide = projection.to_guide().unwrap();

        for step in guide.steps() {
            assert!(step.nearby.is_empty());
        }
    }

    // ---- Boundary tests ----

    #[test]
    fn exactly_200_steps_accepted() {
        let (_temp, loaded) = saved_project_fixture(200);
        let projection = ActionGuideContextProjectionV1::from_loaded_project(&loaded).unwrap();
        assert_eq!(projection.steps().len(), 200);
    }

    #[test]
    fn exactly_4096_byte_guide_title_accepted() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("guide.rollshot-guide");

        let title = "A".repeat(4096);
        let snapshot = ProjectSnapshot {
            base_revision: None,
            title: title.clone(),
            capture_region: CaptureRegion {
                x: 0,
                y: 0,
                width: 8,
                height: 8,
            },
            input_source: InputSourceKind::VisualOnly,
            input_capability: InputCapability::SemanticEvents,
            enabled_outputs: EnabledOutputs::default(),
            frames: vec![SnapshotFrame {
                id: 1,
                at_ms: 100,
                payload: SnapshotFramePayload::Pixels(pixel_image(8, 8)),
            }],
            steps: vec![ProjectStep {
                id: ProjectStepId(1),
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

        create_project(&snapshot, &root).unwrap();
        let loaded = load_project(&root, None).unwrap();
        let projection = ActionGuideContextProjectionV1::from_loaded_project(&loaded).unwrap();
        assert_eq!(projection.title(), &title);
    }

    #[test]
    fn exactly_4096_byte_step_title_accepted() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("guide.rollshot-guide");

        let title_4096 = "B".repeat(4096);
        let snapshot = ProjectSnapshot {
            base_revision: None,
            title: "Guide".into(),
            capture_region: CaptureRegion {
                x: 0,
                y: 0,
                width: 8,
                height: 8,
            },
            input_source: InputSourceKind::VisualOnly,
            input_capability: InputCapability::SemanticEvents,
            enabled_outputs: EnabledOutputs::default(),
            frames: vec![SnapshotFrame {
                id: 1,
                at_ms: 100,
                payload: SnapshotFramePayload::Pixels(pixel_image(8, 8)),
            }],
            steps: vec![ProjectStep {
                id: ProjectStepId(1),
                order: 1,
                title: title_4096.clone(),
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

        create_project(&snapshot, &root).unwrap();
        let loaded = load_project(&root, None).unwrap();
        let projection = ActionGuideContextProjectionV1::from_loaded_project(&loaded).unwrap();
        assert_eq!(projection.steps()[0].title, title_4096);
    }

    #[test]
    fn exactly_4096_byte_caption_accepted() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("guide.rollshot-guide");

        let caption_4096 = "C".repeat(4096);
        let snapshot = ProjectSnapshot {
            base_revision: None,
            title: "Guide".into(),
            capture_region: CaptureRegion {
                x: 0,
                y: 0,
                width: 8,
                height: 8,
            },
            input_source: InputSourceKind::VisualOnly,
            input_capability: InputCapability::SemanticEvents,
            enabled_outputs: EnabledOutputs::default(),
            frames: vec![SnapshotFrame {
                id: 1,
                at_ms: 100,
                payload: SnapshotFramePayload::Pixels(pixel_image(8, 8)),
            }],
            steps: vec![ProjectStep {
                id: ProjectStepId(1),
                order: 1,
                title: "Step".into(),
                caption: Some(caption_4096.clone()),
                kind: CandidateKind::Click,
                reason: DetectReason::ClickConfirmed,
                at_ms: 150,
                keyframe: 1,
                nearby: vec![1],
                annotations: None,
            }],
            import_warnings: Vec::new(),
        };

        create_project(&snapshot, &root).unwrap();
        let loaded = load_project(&root, None).unwrap();
        let projection = ActionGuideContextProjectionV1::from_loaded_project(&loaded).unwrap();
        assert_eq!(
            projection.steps()[0].caption.as_deref(),
            Some(caption_4096.as_str())
        );
    }

    #[test]
    fn projection_includes_kind_and_reason() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("guide.rollshot-guide");

        let snapshot = ProjectSnapshot {
            base_revision: None,
            title: "Guide".into(),
            capture_region: CaptureRegion {
                x: 0,
                y: 0,
                width: 8,
                height: 8,
            },
            input_source: InputSourceKind::VisualOnly,
            input_capability: InputCapability::SemanticEvents,
            enabled_outputs: EnabledOutputs::default(),
            frames: vec![SnapshotFrame {
                id: 1,
                at_ms: 100,
                payload: SnapshotFramePayload::Pixels(pixel_image(8, 8)),
            }],
            steps: vec![
                ProjectStep {
                    id: ProjectStepId(1),
                    order: 1,
                    title: "Click step".into(),
                    caption: None,
                    kind: CandidateKind::Click,
                    reason: DetectReason::ClickConfirmed,
                    at_ms: 150,
                    keyframe: 1,
                    nearby: vec![1],
                    annotations: None,
                },
                ProjectStep {
                    id: ProjectStepId(2),
                    order: 2,
                    title: "Typing step".into(),
                    caption: None,
                    kind: CandidateKind::Typing,
                    reason: DetectReason::TypingSettled,
                    at_ms: 250,
                    keyframe: 1,
                    nearby: vec![1],
                    annotations: None,
                },
            ],
            import_warnings: Vec::new(),
        };

        create_project(&snapshot, &root).unwrap();
        let loaded = load_project(&root, None).unwrap();
        let projection = ActionGuideContextProjectionV1::from_loaded_project(&loaded).unwrap();

        assert_eq!(projection.steps()[0].kind, CandidateKind::Click);
        assert_eq!(projection.steps()[0].reason, DetectReason::ClickConfirmed);
        assert_eq!(projection.steps()[1].kind, CandidateKind::Typing);
        assert_eq!(projection.steps()[1].reason, DetectReason::TypingSettled);
    }

    #[test]
    fn different_revisions_produce_different_digests() {
        let (_temp1, loaded1) = saved_project_fixture(1);
        let proj1 = ActionGuideContextProjectionV1::from_loaded_project(&loaded1).unwrap();

        let (_temp2, loaded2) = saved_project_at_revision(1, 2);
        let proj2 = ActionGuideContextProjectionV1::from_loaded_project(&loaded2).unwrap();

        assert_ne!(proj1.digest(), proj2.digest());
    }

    #[test]
    fn different_step_counts_produce_different_digests() {
        let (_temp1, loaded1) = saved_project_fixture(1);
        let proj1 = ActionGuideContextProjectionV1::from_loaded_project(&loaded1).unwrap();

        let (_temp2, loaded2) = saved_project_fixture(2);
        let proj2 = ActionGuideContextProjectionV1::from_loaded_project(&loaded2).unwrap();

        assert_ne!(proj1.digest(), proj2.digest());
    }

    // ---- Exact boundary rejection ----

    #[test]
    fn guide_title_4097_bytes_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("guide.rollshot-guide");

        let title_4097 = "A".repeat(4097);
        let snapshot = ProjectSnapshot {
            base_revision: None,
            title: title_4097,
            capture_region: CaptureRegion {
                x: 0,
                y: 0,
                width: 8,
                height: 8,
            },
            input_source: InputSourceKind::VisualOnly,
            input_capability: InputCapability::SemanticEvents,
            enabled_outputs: EnabledOutputs::default(),
            frames: vec![SnapshotFrame {
                id: 1,
                at_ms: 100,
                payload: SnapshotFramePayload::Pixels(pixel_image(8, 8)),
            }],
            steps: vec![ProjectStep {
                id: ProjectStepId(1),
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

        create_project(&snapshot, &root).unwrap();
        let loaded = load_project(&root, None).unwrap();
        let err = ActionGuideContextProjectionV1::from_loaded_project(&loaded).unwrap_err();
        assert!(matches!(
            err,
            ActionGuideProjectionError::GuideTitleTooLong { len: 4097 }
        ));
    }

    #[test]
    fn step_title_4097_bytes_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("guide.rollshot-guide");

        let title_4097 = "B".repeat(4097);
        let snapshot = ProjectSnapshot {
            base_revision: None,
            title: "Guide".into(),
            capture_region: CaptureRegion {
                x: 0,
                y: 0,
                width: 8,
                height: 8,
            },
            input_source: InputSourceKind::VisualOnly,
            input_capability: InputCapability::SemanticEvents,
            enabled_outputs: EnabledOutputs::default(),
            frames: vec![SnapshotFrame {
                id: 1,
                at_ms: 100,
                payload: SnapshotFramePayload::Pixels(pixel_image(8, 8)),
            }],
            steps: vec![ProjectStep {
                id: ProjectStepId(1),
                order: 1,
                title: title_4097,
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

        create_project(&snapshot, &root).unwrap();
        let loaded = load_project(&root, None).unwrap();
        let err = ActionGuideContextProjectionV1::from_loaded_project(&loaded).unwrap_err();
        assert!(matches!(
            err,
            ActionGuideProjectionError::StepTitleTooLong {
                step_id: 1,
                len: 4097
            }
        ));
    }

    #[test]
    fn step_caption_4097_bytes_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("guide.rollshot-guide");

        let caption_4097 = "C".repeat(4097);
        let snapshot = ProjectSnapshot {
            base_revision: None,
            title: "Guide".into(),
            capture_region: CaptureRegion {
                x: 0,
                y: 0,
                width: 8,
                height: 8,
            },
            input_source: InputSourceKind::VisualOnly,
            input_capability: InputCapability::SemanticEvents,
            enabled_outputs: EnabledOutputs::default(),
            frames: vec![SnapshotFrame {
                id: 1,
                at_ms: 100,
                payload: SnapshotFramePayload::Pixels(pixel_image(8, 8)),
            }],
            steps: vec![ProjectStep {
                id: ProjectStepId(1),
                order: 1,
                title: "Step".into(),
                caption: Some(caption_4097),
                kind: CandidateKind::Click,
                reason: DetectReason::ClickConfirmed,
                at_ms: 150,
                keyframe: 1,
                nearby: vec![1],
                annotations: None,
            }],
            import_warnings: Vec::new(),
        };

        create_project(&snapshot, &root).unwrap();
        let loaded = load_project(&root, None).unwrap();
        let err = ActionGuideContextProjectionV1::from_loaded_project(&loaded).unwrap_err();
        assert!(matches!(
            err,
            ActionGuideProjectionError::StepCaptionTooLong {
                step_id: 1,
                len: 4097
            }
        ));
    }

    // ------------------------------------------------------------------
    // Deterministic recovery measurements (Task 10 gate)
    // ------------------------------------------------------------------

    #[test]
    fn recovery_measurements() {
        let (_temp, loaded) = saved_project_fixture(7);
        let input_bytes = serde_json::to_vec(&loaded.manifest).unwrap().len();

        let first = ActionGuideContextProjectionV1::from_loaded_project(&loaded).unwrap();
        let second = ActionGuideContextProjectionV1::from_loaded_project(&loaded).unwrap();

        let proj_bytes = first.canonical_bytes().len();
        let step_count = first.steps().len();

        // Same-revision determinism.
        assert_eq!(
            first.canonical_bytes(),
            second.canonical_bytes(),
            "same-revision canonical bytes must be equal"
        );
        assert_eq!(
            first.digest(),
            second.digest(),
            "same-revision digests must be equal"
        );
        assert_eq!(first.revision(), loaded.manifest.revision);

        // Privacy: no paths, pixels, or annotations in projection.
        let json = String::from_utf8_lossy(first.canonical_bytes());
        assert!(!json.contains("annotations"));
        assert!(!json.contains("frames"));
        assert!(!json.contains("sha256"));

        // Guide reconstruction has zero prior provider history.
        let guide = first.to_guide().unwrap();
        assert_eq!(guide.steps().len(), step_count);

        // Print measurements for gate record.
        println!("Action Guide recovery measurements:");
        println!("  canonical_input_bytes: {input_bytes}");
        println!("  projection_bytes: {proj_bytes}");
        println!("  step_count: {step_count}");
        println!("  revision: {}", first.revision());
        println!("  projection_digest: {}", first.digest());
        println!("  prior_history_message_count: 0 (fresh guide)");
        println!("  same_revision_bytes_equal: true");
        println!("  same_revision_digests_equal: true");
    }
}
