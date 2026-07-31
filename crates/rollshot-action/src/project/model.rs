use std::collections::BTreeMap;
use std::sync::Arc;

use image::RgbaImage;
use serde::{Deserialize, Serialize};

use crate::models::{
    CandidateKind, CaptureRegion, DetectReason, FrameId, ImportWarning, InputCapability,
    InputSourceKind, Millis,
};
use crate::motion::asset::ValidatedMotionAsset;
use crate::motion::error::MotionFailureCategory;
use crate::motion::probe::{MotionAudio, MotionCodec};

use super::error::{ProjectError, ProjectErrorCategory};
use rollshot_image_document::{Annotation, AnnotationId};

pub const PROJECT_SCHEMA_VERSION: u32 = 3;

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
    pub annotations: Vec<Annotation>,
    pub explanations: BTreeMap<AnnotationId, String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectStep {
    pub id: ProjectStepId,
    pub order: u32,
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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectManifestV2 {
    pub schema_version: u32,
    pub revision: u64,
    pub title: String,
    pub capture_region: CaptureRegion,
    pub input_source: InputSourceKind,
    pub input_capability: InputCapability,
    pub enabled_outputs: EnabledOutputs,
    pub frames: Vec<ProjectFrame>,
    pub steps: Vec<ProjectStep>,
    pub import_warnings: Vec<ImportWarning>,
}

/// Persisted motion asset metadata. Validated during structure checks;
/// asset-level validation (file exists, digest, probe) happens at load time.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MotionAsset {
    pub relative_path: String,
    pub sha256: String,
    pub duration_ms: u64,
    pub width: u32,
    pub height: u32,
    pub fps_numerator: u32,
    pub fps_denominator: u32,
    pub codec: String,
    pub audio: String,
}

/// Load-time state of a motion asset. Separates "no motion specified" from
/// "motion specified but unavailable" so the Guide can still load.
#[derive(Debug, Clone)]
pub enum MotionAssetLoad {
    /// No motion asset was specified in the manifest.
    None,
    /// Motion asset was specified, validated, and is available.
    Available(ValidatedMotionAsset),
    /// Motion asset was specified but could not be loaded.
    Unavailable(MotionFailureCategory),
}

/// Current manifest schema with motion asset support.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectManifestV3 {
    pub schema_version: u32,
    pub revision: u64,
    pub title: String,
    pub capture_region: CaptureRegion,
    pub input_source: InputSourceKind,
    pub input_capability: InputCapability,
    pub enabled_outputs: EnabledOutputs,
    pub frames: Vec<ProjectFrame>,
    pub steps: Vec<ProjectStep>,
    pub import_warnings: Vec<ImportWarning>,
    pub motion: Option<MotionAsset>,
}

impl From<ProjectManifestV1> for ProjectManifestV2 {
    fn from(v1: ProjectManifestV1) -> Self {
        Self {
            schema_version: 2,
            revision: v1.revision,
            title: v1.title,
            capture_region: v1.capture_region,
            input_source: v1.input_source,
            input_capability: v1.input_capability,
            enabled_outputs: v1.enabled_outputs,
            frames: v1.frames,
            steps: v1.steps,
            import_warnings: Vec::new(),
        }
    }
}

impl From<ProjectManifestV2> for ProjectManifestV3 {
    fn from(v2: ProjectManifestV2) -> Self {
        Self {
            schema_version: PROJECT_SCHEMA_VERSION,
            revision: v2.revision,
            title: v2.title,
            capture_region: v2.capture_region,
            input_source: v2.input_source,
            input_capability: v2.input_capability,
            enabled_outputs: v2.enabled_outputs,
            frames: v2.frames,
            steps: v2.steps,
            import_warnings: v2.import_warnings,
            motion: None,
        }
    }
}

impl MotionAsset {
    /// The only canonical relative path for a motion asset.
    pub const CANONICAL_PATH: &'static str = "assets/motion/recording.mp4";

    /// Validate structural invariants of the persisted motion asset.
    pub fn validate_structure(&self) -> Result<(), ProjectError> {
        if self.relative_path != Self::CANONICAL_PATH {
            return Err(ProjectError::invalid_manifest(
                ProjectErrorCategory::InvalidMotion,
                None,
                None,
            ));
        }
        if !is_canonical_sha256(&self.sha256) {
            return Err(ProjectError::invalid_manifest(
                ProjectErrorCategory::InvalidMotion,
                None,
                None,
            ));
        }
        if self.duration_ms == 0
            || self.width == 0
            || self.height == 0
            || self.fps_numerator == 0
            || self.fps_denominator == 0
        {
            return Err(ProjectError::invalid_manifest(
                ProjectErrorCategory::InvalidMotion,
                None,
                None,
            ));
        }
        if self.codec != MotionCodec::H264.as_str() {
            return Err(ProjectError::invalid_manifest(
                ProjectErrorCategory::InvalidMotion,
                None,
                None,
            ));
        }
        if self.audio != MotionAudio::None.as_str() {
            return Err(ProjectError::invalid_manifest(
                ProjectErrorCategory::InvalidMotion,
                None,
                None,
            ));
        }
        Ok(())
    }
}

fn is_canonical_sha256(digest: &str) -> bool {
    digest.len() == 64
        && digest
            .as_bytes()
            .iter()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(byte))
}

#[derive(Clone)]
pub enum SnapshotFramePayload {
    Pixels(Arc<RgbaImage>),
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
    pub import_warnings: Vec<ImportWarning>,
    pub motion: Option<ValidatedMotionAsset>,
}

#[derive(Debug, Clone)]
pub struct LoadedProject {
    pub root: std::path::PathBuf,
    pub manifest: ProjectManifestV3,
    pub motion: MotionAssetLoad,
}

#[derive(Debug, Clone)]
pub struct ProjectCommit {
    pub root: std::path::PathBuf,
    pub manifest: ProjectManifestV3,
    pub motion: Option<ValidatedMotionAsset>,
}

#[cfg(test)]
mod tests {
    use super::*;

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

    #[test]
    fn manifest_rejects_unknown_nested_capture_region_field() {
        let json = serde_json::json!({
            "schema_version": 1,
            "revision": 1,
            "title": "Guide",
            "capture_region": {
                "x": 0,
                "y": 0,
                "width": 8,
                "height": 8,
                "surprise": true
            },
            "input_source": "visual-only",
            "input_capability": { "visual-only": { "reason": "source-start-failed" } },
            "enabled_outputs": { "storyboard": false, "gif": false, "mp4": false },
            "frames": [],
            "steps": []
        });

        let error = serde_json::from_value::<ProjectManifestV1>(json).unwrap_err();
        assert!(error.to_string().contains("unknown field"));
    }

    #[test]
    fn manifest_rejects_unknown_input_capability_field() {
        let mut json = valid_minimal_manifest_json();
        json["input_capability"]["visual-only"]["surprise"] = serde_json::json!(true);

        let error = serde_json::from_value::<ProjectManifestV1>(json).unwrap_err();
        assert!(error.to_string().contains("unknown field"));
    }

    #[test]
    fn annotations_reject_unknown_variant_and_geometry_fields() {
        let json = serde_json::json!({
            "annotations": [{
                "opaque_redaction": {
                    "id": 1,
                    "bounds": {
                        "x": 0.0,
                        "y": 0.0,
                        "width": 4.0,
                        "height": 4.0,
                        "surprise": true
                    },
                    "unexpected_variant_field": true
                }
            }],
            "explanations": {}
        });

        let error = serde_json::from_value::<PersistedStepAnnotations>(json).unwrap_err();
        assert!(error.to_string().contains("unknown field"));
    }

    fn valid_minimal_manifest_json() -> serde_json::Value {
        serde_json::json!({
            "schema_version": 1,
            "revision": 1,
            "title": "Guide",
            "capture_region": { "x": 0, "y": 0, "width": 8, "height": 8 },
            "input_source": "visual-only",
            "input_capability": { "visual-only": { "reason": "source-start-failed" } },
            "enabled_outputs": { "storyboard": false, "gif": false, "mp4": false },
            "frames": [],
            "steps": []
        })
    }

    #[test]
    fn manifest_accepts_valid_minimal_fixture() {
        let json = serde_json::json!({
            "schema_version": 1,
            "revision": 1,
            "title": "Guide",
            "capture_region": { "x": 0, "y": 0, "width": 8, "height": 8 },
            "input_source": "visual-only",
            "input_capability": { "visual-only": { "reason": "source-start-failed" } },
            "enabled_outputs": { "storyboard": false, "gif": false, "mp4": false },
            "frames": [],
            "steps": []
        });
        let manifest = serde_json::from_value::<ProjectManifestV1>(json).unwrap();
        assert_eq!(manifest.schema_version, 1);
        assert_eq!(manifest.revision, 1);
        assert_eq!(manifest.title, "Guide");
    }

    #[test]
    fn manifest_round_trips() {
        let json = serde_json::json!({
            "schema_version": 1,
            "revision": 42,
            "title": "My Guide",
            "capture_region": { "x": 10, "y": 20, "width": 1920, "height": 1080 },
            "input_source": "linux-evdev",
            "input_capability": "semantic-events",
            "enabled_outputs": { "storyboard": true, "gif": false, "mp4": true },
            "frames": [
                { "id": 1, "at_ms": 100, "sha256": "abc123", "width": 1920, "height": 1080 }
            ],
            "steps": [
                {
                    "id": 1,
                    "order": 0,
                    "title": "Click",
                    "caption": null,
                    "kind": "click",
                    "reason": "click-confirmed",
                    "at_ms": 150,
                    "keyframe": 1,
                    "nearby": [1],
                    "annotations": null
                }
            ]
        });
        let manifest: ProjectManifestV1 = serde_json::from_value(json).unwrap();
        let serialized = serde_json::to_string(&manifest).unwrap();
        let round_tripped: ProjectManifestV1 = serde_json::from_str(&serialized).unwrap();
        assert_eq!(manifest, round_tripped);
    }

    #[test]
    fn enabled_outputs_defaults() {
        let outputs = EnabledOutputs::default();
        assert!(!outputs.storyboard);
        assert!(!outputs.gif);
        assert!(!outputs.mp4);
    }

    #[test]
    fn project_step_id_is_transparent_u64() {
        let id = ProjectStepId(42);
        assert_eq!(id.0, 42);
        let json = serde_json::to_string(&id).unwrap();
        assert_eq!(json, "42");
        let back: ProjectStepId = serde_json::from_str(&json).unwrap();
        assert_eq!(back, id);
    }

    #[test]
    fn persisted_step_annotations_rejects_unknown_fields() {
        let json = serde_json::json!({
            "annotations": [],
            "explanations": {},
            "extra": true
        });
        let error = serde_json::from_value::<PersistedStepAnnotations>(json).unwrap_err();
        assert!(error.to_string().contains("unknown field"));
    }

    #[test]
    fn project_frame_rejects_unknown_fields() {
        let json = serde_json::json!({
            "id": 1,
            "at_ms": 100,
            "sha256": "abc",
            "width": 800,
            "height": 600,
            "extra": true
        });
        let error = serde_json::from_value::<ProjectFrame>(json).unwrap_err();
        assert!(error.to_string().contains("unknown field"));
    }

    #[test]
    fn project_step_rejects_unknown_fields() {
        let json = serde_json::json!({
            "id": 1,
            "order": 0,
            "title": "Step",
            "caption": null,
            "kind": "click",
            "reason": "click-confirmed",
            "at_ms": 100,
            "keyframe": 1,
            "nearby": [],
            "annotations": null,
            "extra": true
        });
        let error = serde_json::from_value::<ProjectStep>(json).unwrap_err();
        assert!(error.to_string().contains("unknown field"));
    }

    // ---- Schema v3 model RED tests ----

    #[test]
    fn v3_manifest_rejects_unknown_fields() {
        let json = serde_json::json!({
            "schema_version": 3,
            "revision": 1,
            "title": "Guide",
            "capture_region": { "x": 0, "y": 0, "width": 8, "height": 8 },
            "input_source": "visual-only",
            "input_capability": "semantic-events",
            "enabled_outputs": { "storyboard": false, "gif": false, "mp4": false },
            "frames": [],
            "steps": [],
            "import_warnings": [],
            "motion": null,
            "surprise": true
        });
        let error = serde_json::from_value::<ProjectManifestV3>(json).unwrap_err();
        assert!(error.to_string().contains("unknown field"));
    }

    #[test]
    fn v3_manifest_round_trips_with_motion_none() {
        let json = serde_json::json!({
            "schema_version": 3,
            "revision": 1,
            "title": "Guide",
            "capture_region": { "x": 0, "y": 0, "width": 8, "height": 8 },
            "input_source": "visual-only",
            "input_capability": "semantic-events",
            "enabled_outputs": { "storyboard": false, "gif": false, "mp4": false },
            "frames": [],
            "steps": [],
            "import_warnings": [],
            "motion": null
        });
        let manifest: ProjectManifestV3 = serde_json::from_value(json).unwrap();
        let serialized = serde_json::to_string(&manifest).unwrap();
        let round_tripped: ProjectManifestV3 = serde_json::from_str(&serialized).unwrap();
        assert_eq!(manifest, round_tripped);
        assert!(round_tripped.motion.is_none());
    }

    #[test]
    fn v3_manifest_round_trips_with_motion_some() {
        let json = serde_json::json!({
            "schema_version": 3,
            "revision": 1,
            "title": "Guide",
            "capture_region": { "x": 0, "y": 0, "width": 8, "height": 8 },
            "input_source": "visual-only",
            "input_capability": "semantic-events",
            "enabled_outputs": { "storyboard": false, "gif": false, "mp4": false },
            "frames": [],
            "steps": [],
            "import_warnings": [],
            "motion": {
                "relative_path": "assets/motion/recording.mp4",
                "sha256": "a".repeat(64),
                "duration_ms": 1000,
                "width": 1920,
                "height": 1080,
                "fps_numerator": 30,
                "fps_denominator": 1,
                "codec": "h264",
                "audio": "none"
            }
        });
        let manifest: ProjectManifestV3 = serde_json::from_value(json).unwrap();
        let serialized = serde_json::to_string(&manifest).unwrap();
        let round_tripped: ProjectManifestV3 = serde_json::from_str(&serialized).unwrap();
        assert_eq!(manifest, round_tripped);
        let m = round_tripped.motion.unwrap();
        assert_eq!(m.relative_path, "assets/motion/recording.mp4");
        assert_eq!(m.duration_ms, 1000);
        assert_eq!(m.codec, "h264");
    }

    #[test]
    fn motion_asset_rejects_unknown_fields() {
        let json = serde_json::json!({
            "relative_path": "assets/motion/recording.mp4",
            "sha256": "a".repeat(64),
            "duration_ms": 1000,
            "width": 1920,
            "height": 1080,
            "fps_numerator": 30,
            "fps_denominator": 1,
            "codec": "h264",
            "audio": "none",
            "extra": true
        });
        let error = serde_json::from_value::<MotionAsset>(json).unwrap_err();
        assert!(error.to_string().contains("unknown field"));
    }
}
