use std::collections::BTreeMap;
use std::sync::Arc;

use image::RgbaImage;
use serde::{Deserialize, Serialize};

use crate::models::{
    CandidateKind, CaptureRegion, DetectReason, FrameId, InputCapability, InputSourceKind, Millis,
};
use rollshot_image_document::{Annotation, AnnotationId};

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
        assert_eq!(manifest.schema_version, PROJECT_SCHEMA_VERSION);
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
}
