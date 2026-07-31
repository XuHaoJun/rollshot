use std::collections::BTreeSet;

use super::error::{ProjectError, ProjectErrorCategory};
use super::model::{
    PersistedStepAnnotations, ProjectFrame, ProjectManifestV3, ProjectSnapshot, ProjectStep,
    SnapshotFrame, SnapshotFramePayload, PROJECT_SCHEMA_VERSION,
};
use crate::models::{CaptureRegion, FrameId, ImportWarning};
use rollshot_image_document::ImageDocument;

/// Hash-free frame view: `Pixels` payloads have no digest until encoding.
struct FrameMeta {
    id: FrameId,
    sha256: Option<String>,
    width: u32,
    height: u32,
}

impl FrameMeta {
    fn from_project_frame(frame: &ProjectFrame) -> Self {
        Self {
            id: frame.id,
            sha256: Some(frame.sha256.clone()),
            width: frame.width,
            height: frame.height,
        }
    }

    fn from_snapshot_frame(frame: &SnapshotFrame) -> Self {
        let (width, height) = match &frame.payload {
            SnapshotFramePayload::Pixels(img) => img.dimensions(),
            SnapshotFramePayload::ExistingAsset { width, height, .. } => (*width, *height),
        };
        Self {
            id: frame.id,
            sha256: match &frame.payload {
                SnapshotFramePayload::ExistingAsset { sha256, .. } => Some(sha256.clone()),
                SnapshotFramePayload::Pixels(_) => None,
            },
            width,
            height,
        }
    }
}

pub fn validate_manifest_structure(manifest: &ProjectManifestV3) -> Result<(), ProjectError> {
    if manifest.schema_version != PROJECT_SCHEMA_VERSION {
        return Err(ProjectError::UnsupportedSchema {
            path: None,
            version: manifest.schema_version,
        });
    }
    if manifest.revision == 0 {
        return Err(ProjectError::invalid_manifest(
            ProjectErrorCategory::ZeroRevision,
            None,
            None,
        ));
    }
    validate_import_warnings(&manifest.import_warnings)?;
    if let Some(ref motion) = manifest.motion {
        motion.validate_structure()?;
    }
    let frames = manifest
        .frames
        .iter()
        .map(FrameMeta::from_project_frame)
        .collect::<Vec<_>>();
    validate_common(manifest.capture_region, &frames, &manifest.steps)
}

pub fn validate_snapshot_structure(snapshot: &ProjectSnapshot) -> Result<(), ProjectError> {
    validate_import_warnings(&snapshot.import_warnings)?;
    let frames = snapshot
        .frames
        .iter()
        .map(FrameMeta::from_snapshot_frame)
        .collect::<Vec<_>>();
    validate_common(snapshot.capture_region, &frames, &snapshot.steps)
}

fn validate_common(
    capture_region: CaptureRegion,
    frames: &[FrameMeta],
    steps: &[ProjectStep],
) -> Result<(), ProjectError> {
    if capture_region.width == 0 || capture_region.height == 0 {
        return Err(ProjectError::invalid_manifest(
            ProjectErrorCategory::ZeroCaptureRegion,
            None,
            None,
        ));
    }
    if frames.is_empty() {
        return Err(ProjectError::invalid_manifest(
            ProjectErrorCategory::EmptyFrames,
            None,
            None,
        ));
    }
    if steps.is_empty() {
        return Err(ProjectError::invalid_manifest(
            ProjectErrorCategory::EmptySteps,
            None,
            None,
        ));
    }

    let mut frame_ids = BTreeSet::new();
    for frame in frames {
        if !frame_ids.insert(frame.id) {
            return Err(ProjectError::invalid_manifest(
                ProjectErrorCategory::DuplicateFrameId,
                None,
                Some(frame.id),
            ));
        }
        if frame.width != capture_region.width || frame.height != capture_region.height {
            return Err(ProjectError::invalid_manifest(
                ProjectErrorCategory::FrameDimensionMismatch,
                None,
                Some(frame.id),
            ));
        }
        if frame
            .sha256
            .as_deref()
            .is_some_and(|digest| !is_canonical_sha256(digest))
        {
            return Err(ProjectError::InvalidAsset {
                category: ProjectErrorCategory::InvalidAsset,
                frame_id: frame.id,
            });
        }
    }

    let mut step_ids = BTreeSet::new();
    for (offset, step) in steps.iter().enumerate() {
        let expected_order = (offset as u32) + 1;
        if step.order != expected_order {
            return Err(ProjectError::invalid_manifest(
                ProjectErrorCategory::NonContiguousOrder,
                Some(step.id.0),
                None,
            ));
        }
        if step.id.0 == 0 {
            return Err(ProjectError::invalid_manifest(
                ProjectErrorCategory::EmptySteps,
                Some(0),
                None,
            ));
        }
        if !step_ids.insert(step.id.0) {
            return Err(ProjectError::invalid_manifest(
                ProjectErrorCategory::DuplicateStepId,
                Some(step.id.0),
                None,
            ));
        }

        if !frame_ids.contains(&step.keyframe) {
            return Err(ProjectError::invalid_manifest(
                ProjectErrorCategory::MissingKeyframe,
                Some(step.id.0),
                Some(step.keyframe),
            ));
        }
        if !step.nearby.contains(&step.keyframe) {
            return Err(ProjectError::invalid_manifest(
                ProjectErrorCategory::KeyframeNotNearby,
                Some(step.id.0),
                Some(step.keyframe),
            ));
        }

        let mut nearby_ids = BTreeSet::new();
        for &nid in &step.nearby {
            if !nearby_ids.insert(nid) {
                return Err(ProjectError::invalid_manifest(
                    ProjectErrorCategory::DuplicateNearbyId,
                    Some(step.id.0),
                    Some(nid),
                ));
            }
            if !frame_ids.contains(&nid) {
                return Err(ProjectError::invalid_manifest(
                    ProjectErrorCategory::MissingNearbyFrame,
                    Some(step.id.0),
                    Some(nid),
                ));
            }
        }

        if let Some(ref annotations) = step.annotations {
            validate_annotations(capture_region, frames, step, annotations)?;
        }
    }

    Ok(())
}

fn is_canonical_sha256(digest: &str) -> bool {
    digest.len() == 64
        && digest
            .as_bytes()
            .iter()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(byte))
}

fn validate_import_warnings(warnings: &[ImportWarning]) -> Result<(), ProjectError> {
    if warnings.len() > 2 {
        return Err(ProjectError::invalid_manifest(
            ProjectErrorCategory::DuplicateImportWarning,
            None,
            None,
        ));
    }
    let mut seen = std::collections::BTreeSet::new();
    for w in warnings {
        if !seen.insert(w) {
            return Err(ProjectError::invalid_manifest(
                ProjectErrorCategory::DuplicateImportWarning,
                None,
                None,
            ));
        }
    }
    Ok(())
}

fn validate_annotations(
    _capture_region: CaptureRegion,
    frames: &[FrameMeta],
    step: &ProjectStep,
    annotations: &PersistedStepAnnotations,
) -> Result<(), ProjectError> {
    let annotation_ids: BTreeSet<u64> = annotations.annotations.iter().map(|a| a.id().0).collect();

    for explanation_id in annotations.explanations.keys() {
        if !annotation_ids.contains(&explanation_id.0) {
            return Err(ProjectError::invalid_manifest(
                ProjectErrorCategory::MissingExplanationAnnotation,
                Some(step.id.0),
                None,
            ));
        }
    }

    let keyframe_meta = frames
        .iter()
        .find(|f| f.id == step.keyframe)
        .expect("keyframe existence already validated");

    ImageDocument::validate_persisted_annotations(
        keyframe_meta.width,
        keyframe_meta.height,
        &annotations.annotations,
    )
    .map_err(|_| {
        ProjectError::invalid_manifest(
            ProjectErrorCategory::AnnotationValidationFailed,
            Some(step.id.0),
            None,
        )
    })?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use std::sync::Arc;

    use image::RgbaImage;

    use super::super::model::{
        EnabledOutputs, ProjectFrame, ProjectStepId, SnapshotFrame, SnapshotFramePayload,
        PROJECT_SCHEMA_VERSION,
    };
    use crate::models::{
        CandidateKind, CaptureRegion, DetectReason, InputCapability, InputSourceKind,
    };
    use rollshot_image_document::AnnotationId;

    fn valid_manifest() -> ProjectManifestV3 {
        ProjectManifestV3 {
            schema_version: PROJECT_SCHEMA_VERSION,
            revision: 1,
            title: String::new(),
            capture_region: CaptureRegion {
                x: 0,
                y: 0,
                width: 8,
                height: 8,
            },
            input_source: InputSourceKind::VisualOnly,
            input_capability: InputCapability::SemanticEvents,
            enabled_outputs: EnabledOutputs::default(),
            frames: vec![ProjectFrame {
                id: 1,
                at_ms: 100,
                sha256: "a".repeat(64),
                width: 8,
                height: 8,
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
            motion: None,
        }
    }

    fn valid_snapshot() -> ProjectSnapshot {
        ProjectSnapshot {
            base_revision: Some(1),
            title: String::new(),
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
                payload: SnapshotFramePayload::Pixels(Arc::new(RgbaImage::new(8, 8))),
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
            motion: None,
        }
    }

    // ---- Manifest validation ----

    #[test]
    fn manifest_rejects_wrong_schema_version() {
        let mut manifest = valid_manifest();
        manifest.schema_version = 99;
        let error = validate_manifest_structure(&manifest).unwrap_err();
        assert_eq!(error.category(), "unsupported-schema");
    }

    #[test]
    fn manifest_rejects_zero_revision() {
        let mut manifest = valid_manifest();
        manifest.revision = 0;
        let error = validate_manifest_structure(&manifest).unwrap_err();
        assert_eq!(error.category(), "zero-revision");
    }

    #[test]
    fn manifest_rejects_non_canonical_asset_digest() {
        for digest in [
            "abc".to_string(),
            "A".repeat(64),
            format!("{}../escape", "a".repeat(55)),
            format!("{}/escape", "a".repeat(63)),
        ] {
            let mut manifest = valid_manifest();
            manifest.frames[0].sha256 = digest;
            let error = validate_manifest_structure(&manifest).unwrap_err();
            assert_eq!(error.category(), "invalid-asset");
        }
    }

    #[test]
    fn manifest_rejects_no_steps() {
        let mut manifest = valid_manifest();
        manifest.steps = vec![];
        let error = validate_manifest_structure(&manifest).unwrap_err();
        assert_eq!(error.category(), "empty-steps");
    }

    #[test]
    fn manifest_rejects_duplicate_frame_ids() {
        let mut manifest = valid_manifest();
        manifest.steps[0].nearby = vec![1, 2];
        manifest.steps[0].keyframe = 1;
        manifest.frames.push(ProjectFrame {
            id: 1,
            at_ms: 200,
            sha256: "b".repeat(64),
            width: 8,
            height: 8,
        });
        manifest.frames.push(ProjectFrame {
            id: 2,
            at_ms: 300,
            sha256: "c".repeat(64),
            width: 8,
            height: 8,
        });
        let error = validate_manifest_structure(&manifest).unwrap_err();
        assert_eq!(error.category(), "duplicate-frame-id");
    }

    #[test]
    fn manifest_rejects_duplicate_step_ids() {
        let mut manifest = valid_manifest();
        manifest.steps.push(ProjectStep {
            id: ProjectStepId(1),
            order: 2,
            title: "Step 2".into(),
            caption: None,
            kind: CandidateKind::Click,
            reason: DetectReason::ClickConfirmed,
            at_ms: 250,
            keyframe: 1,
            nearby: vec![1],
            annotations: None,
        });
        let error = validate_manifest_structure(&manifest).unwrap_err();
        assert_eq!(error.category(), "duplicate-step-id");
    }

    #[test]
    fn manifest_rejects_non_contiguous_order() {
        let mut manifest = valid_manifest();
        manifest.steps[0].order = 5;
        let error = validate_manifest_structure(&manifest).unwrap_err();
        assert_eq!(error.category(), "non-contiguous-order");
    }

    #[test]
    fn manifest_rejects_missing_keyframe() {
        let mut manifest = valid_manifest();
        manifest.steps[0].keyframe = 99;
        manifest.steps[0].nearby = vec![99];
        let error = validate_manifest_structure(&manifest).unwrap_err();
        assert_eq!(error.category(), "missing-keyframe");
    }

    #[test]
    fn keyframe_must_be_present_in_nearby() {
        let mut manifest = valid_manifest();
        manifest.steps[0].keyframe = 1;
        manifest.steps[0].nearby = vec![7, 8];
        manifest.frames.push(ProjectFrame {
            id: 7,
            at_ms: 200,
            sha256: "d".repeat(64),
            width: 8,
            height: 8,
        });
        manifest.frames.push(ProjectFrame {
            id: 8,
            at_ms: 300,
            sha256: "e".repeat(64),
            width: 8,
            height: 8,
        });
        let error = validate_manifest_structure(&manifest).unwrap_err();
        assert_eq!(error.category(), "keyframe-not-nearby");
    }

    #[test]
    fn manifest_rejects_duplicate_nearby_ids() {
        let mut manifest = valid_manifest();
        manifest.steps[0].nearby = vec![1, 1];
        let error = validate_manifest_structure(&manifest).unwrap_err();
        assert_eq!(error.category(), "duplicate-nearby-id");
    }

    #[test]
    fn manifest_rejects_missing_nearby_frame() {
        let mut manifest = valid_manifest();
        manifest.steps[0].nearby = vec![1, 99];
        let error = validate_manifest_structure(&manifest).unwrap_err();
        assert_eq!(error.category(), "missing-nearby-frame");
    }

    #[test]
    fn manifest_rejects_zero_capture_region() {
        let mut manifest = valid_manifest();
        manifest.capture_region = CaptureRegion {
            x: 0,
            y: 0,
            width: 0,
            height: 0,
        };
        let error = validate_manifest_structure(&manifest).unwrap_err();
        assert_eq!(error.category(), "zero-capture-region");
    }

    #[test]
    fn manifest_rejects_frame_dimension_mismatch() {
        let mut manifest = valid_manifest();
        manifest.frames[0].width = 16;
        manifest.frames[0].height = 16;
        let error = validate_manifest_structure(&manifest).unwrap_err();
        assert_eq!(error.category(), "frame-dimension-mismatch");
    }

    #[test]
    fn manifest_rejects_missing_explanation_annotation() {
        let mut manifest = valid_manifest();
        manifest.steps[0].annotations = Some(PersistedStepAnnotations {
            annotations: vec![],
            explanations: BTreeMap::from([(AnnotationId(1), "missing".into())]),
        });
        let error = validate_manifest_structure(&manifest).unwrap_err();
        assert_eq!(error.category(), "missing-explanation-annotation");
    }

    #[test]
    fn manifest_accepts_empty_title() {
        let manifest = valid_manifest();
        assert_eq!(manifest.title, "");
        validate_manifest_structure(&manifest).unwrap();
    }

    #[test]
    fn manifest_accepts_valid_fixture() {
        let manifest = valid_manifest();
        validate_manifest_structure(&manifest).unwrap();
    }

    // ---- Snapshot validation ----

    #[test]
    fn snapshot_rejects_no_steps() {
        let mut snapshot = valid_snapshot();
        snapshot.steps = vec![];
        let error = validate_snapshot_structure(&snapshot).unwrap_err();
        assert_eq!(error.category(), "empty-steps");
    }

    #[test]
    fn snapshot_rejects_duplicate_frame_ids() {
        let mut snapshot = valid_snapshot();
        snapshot.steps[0].nearby = vec![1, 2];
        snapshot.steps[0].keyframe = 1;
        snapshot.frames.push(SnapshotFrame {
            id: 1,
            at_ms: 200,
            payload: SnapshotFramePayload::Pixels(Arc::new(RgbaImage::new(8, 8))),
        });
        snapshot.frames.push(SnapshotFrame {
            id: 2,
            at_ms: 300,
            payload: SnapshotFramePayload::Pixels(Arc::new(RgbaImage::new(8, 8))),
        });
        let error = validate_snapshot_structure(&snapshot).unwrap_err();
        assert_eq!(error.category(), "duplicate-frame-id");
    }

    #[test]
    fn snapshot_rejects_duplicate_step_ids() {
        let mut snapshot = valid_snapshot();
        snapshot.steps.push(ProjectStep {
            id: ProjectStepId(1),
            order: 2,
            title: "Step 2".into(),
            caption: None,
            kind: CandidateKind::Click,
            reason: DetectReason::ClickConfirmed,
            at_ms: 250,
            keyframe: 1,
            nearby: vec![1],
            annotations: None,
        });
        let error = validate_snapshot_structure(&snapshot).unwrap_err();
        assert_eq!(error.category(), "duplicate-step-id");
    }

    #[test]
    fn snapshot_rejects_non_contiguous_order() {
        let mut snapshot = valid_snapshot();
        snapshot.steps[0].order = 3;
        let error = validate_snapshot_structure(&snapshot).unwrap_err();
        assert_eq!(error.category(), "non-contiguous-order");
    }

    #[test]
    fn snapshot_rejects_missing_keyframe() {
        let mut snapshot = valid_snapshot();
        snapshot.steps[0].keyframe = 99;
        snapshot.steps[0].nearby = vec![99];
        let error = validate_snapshot_structure(&snapshot).unwrap_err();
        assert_eq!(error.category(), "missing-keyframe");
    }

    #[test]
    fn snapshot_rejects_keyframe_not_nearby() {
        let mut snapshot = valid_snapshot();
        snapshot.steps[0].nearby = vec![2];
        snapshot.frames.push(SnapshotFrame {
            id: 2,
            at_ms: 200,
            payload: SnapshotFramePayload::Pixels(Arc::new(RgbaImage::new(8, 8))),
        });
        let error = validate_snapshot_structure(&snapshot).unwrap_err();
        assert_eq!(error.category(), "keyframe-not-nearby");
    }

    #[test]
    fn snapshot_rejects_duplicate_nearby_ids() {
        let mut snapshot = valid_snapshot();
        snapshot.steps[0].nearby = vec![1, 1];
        let error = validate_snapshot_structure(&snapshot).unwrap_err();
        assert_eq!(error.category(), "duplicate-nearby-id");
    }

    #[test]
    fn snapshot_rejects_frame_dimension_mismatch() {
        let mut snapshot = valid_snapshot();
        snapshot.steps[0].keyframe = 2;
        snapshot.steps[0].nearby = vec![2];
        snapshot.frames.push(SnapshotFrame {
            id: 2,
            at_ms: 200,
            payload: SnapshotFramePayload::Pixels(Arc::new(RgbaImage::new(16, 16))),
        });
        let error = validate_snapshot_structure(&snapshot).unwrap_err();
        assert_eq!(error.category(), "frame-dimension-mismatch");
    }

    #[test]
    fn snapshot_rejects_zero_capture_region() {
        let mut snapshot = valid_snapshot();
        snapshot.capture_region = CaptureRegion {
            x: 0,
            y: 0,
            width: 0,
            height: 0,
        };
        let error = validate_snapshot_structure(&snapshot).unwrap_err();
        assert_eq!(error.category(), "zero-capture-region");
    }

    #[test]
    fn final_step_deletion_empty_snapshot() {
        let mut snapshot = valid_snapshot();
        snapshot.steps = vec![];
        let error = validate_snapshot_structure(&snapshot).unwrap_err();
        assert_eq!(error.category(), "empty-steps");
    }

    #[test]
    fn snapshot_accepts_valid_fixture() {
        let snapshot = valid_snapshot();
        validate_snapshot_structure(&snapshot).unwrap();
    }

    // ---- Motion structure validation RED tests ----

    use super::super::model::MotionAsset;

    fn valid_motion_asset() -> MotionAsset {
        MotionAsset {
            relative_path: "assets/motion/recording.mp4".into(),
            sha256: "a".repeat(64),
            duration_ms: 1000,
            width: 1920,
            height: 1080,
            fps_numerator: 30,
            fps_denominator: 1,
            codec: "h264".into(),
            audio: "none".into(),
        }
    }

    #[test]
    fn motion_invalid_relative_path_rejected() {
        let motion = MotionAsset {
            relative_path: "../escape".into(),
            ..valid_motion_asset()
        };
        let error = motion.validate_structure().unwrap_err();
        assert_eq!(error.category(), "invalid-motion");
    }

    #[test]
    fn motion_absolute_path_rejected() {
        let motion = MotionAsset {
            relative_path: "/etc/passwd".into(),
            ..valid_motion_asset()
        };
        let error = motion.validate_structure().unwrap_err();
        assert_eq!(error.category(), "invalid-motion");
    }

    #[test]
    fn motion_backslash_path_rejected() {
        let motion = MotionAsset {
            relative_path: "assets\\\\motion\\\\recording.mp4".into(),
            ..valid_motion_asset()
        };
        let error = motion.validate_structure().unwrap_err();
        assert_eq!(error.category(), "invalid-motion");
    }

    #[test]
    fn motion_non_canonical_sha256_rejected() {
        let motion = MotionAsset {
            sha256: "NOT-HEX".into(),
            ..valid_motion_asset()
        };
        let error = motion.validate_structure().unwrap_err();
        assert_eq!(error.category(), "invalid-motion");
    }

    #[test]
    fn motion_zero_dimensions_rejected() {
        let motion = MotionAsset {
            width: 0,
            ..valid_motion_asset()
        };
        let error = motion.validate_structure().unwrap_err();
        assert_eq!(error.category(), "invalid-motion");
    }

    #[test]
    fn motion_zero_fps_rejected() {
        let motion = MotionAsset {
            fps_numerator: 0,
            ..valid_motion_asset()
        };
        let error = motion.validate_structure().unwrap_err();
        assert_eq!(error.category(), "invalid-motion");
    }

    #[test]
    fn motion_zero_duration_rejected() {
        let motion = MotionAsset {
            duration_ms: 0,
            ..valid_motion_asset()
        };
        let error = motion.validate_structure().unwrap_err();
        assert_eq!(error.category(), "invalid-motion");
    }

    #[test]
    fn motion_unknown_codec_rejected() {
        let motion = MotionAsset {
            codec: "vp9".into(),
            ..valid_motion_asset()
        };
        let error = motion.validate_structure().unwrap_err();
        assert_eq!(error.category(), "invalid-motion");
    }

    #[test]
    fn motion_unknown_audio_rejected() {
        let motion = MotionAsset {
            audio: "aac".into(),
            ..valid_motion_asset()
        };
        let error = motion.validate_structure().unwrap_err();
        assert_eq!(error.category(), "invalid-motion");
    }

    #[test]
    fn motion_valid_asset_accepted() {
        valid_motion_asset().validate_structure().unwrap();
    }
}
