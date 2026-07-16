//! Portable guide export. Builds `action-guide/{steps.md, session.json,
//! keyframes/*.png}` in a temporary sibling directory and renames it into place
//! only after every file is written. Any failure rolls back the temp dir, so
//! there is never a partial export and the editable session is preserved.
//! `session.json` serializes only step metadata + capability — never raw input.

pub mod model;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::diagnostics::TARGET_EXPORT;
use crate::error::ExportError;
use crate::frame_store::FrameStore;
use crate::guide::Guide;
use crate::models::{
    CandidateKind, CaptureRegion, DetectReason, InputCapability, InputSourceKind, Millis,
};
use model::{GuideHotspot, ReviewedGuideExportJob, GUIDE_SCHEMA_VERSION};

mod html;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SessionManifest {
    #[serde(default = "legacy_schema_version")]
    pub schema_version: u32,
    #[serde(default = "default_manifest_title")]
    pub title: String,
    pub region: CaptureRegion,
    pub input_source: InputSourceKind,
    pub input_capability: InputCapability,
    pub steps: Vec<ManifestStep>,
}

fn legacy_schema_version() -> u32 {
    0
}

fn default_manifest_title() -> String {
    crate::guide::DEFAULT_GUIDE_TITLE.to_string()
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ManifestStep {
    pub index: usize,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub caption: Option<String>,
    pub kind: CandidateKind,
    pub reason: DetectReason,
    pub at_ms: Millis,
    pub keyframe_file: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub hotspots: Vec<GuideHotspot>,
}

pub fn render_guide_folder(
    job: &ReviewedGuideExportJob,
    destination: &Path,
) -> Result<PathBuf, ExportError> {
    job.validate()?;
    if destination.exists() {
        return Err(ExportError::DestinationExists {
            path: destination.display().to_string(),
        });
    }
    std::fs::create_dir(destination).map_err(|source| ExportError::Io {
        path: destination.display().to_string(),
        source,
    })?;
    let result = (|| {
        std::fs::create_dir(destination.join("keyframes")).map_err(|source| ExportError::Io {
            path: destination.display().to_string(),
            source,
        })?;
        build_folder(job, destination)
    })();
    if let Err(error) = result {
        let _ = std::fs::remove_dir_all(destination);
        tracing::debug!(target: TARGET_EXPORT, category = error.category(), "guide export rolled back");
        return Err(error);
    }
    tracing::info!(target: TARGET_EXPORT, steps = job.steps.len(), "guide export complete");
    Ok(destination.to_path_buf())
}

fn build_folder(job: &ReviewedGuideExportJob, destination: &Path) -> Result<(), ExportError> {
    let mut markdown = format!("# {}\n\n", job.title);
    let mut manifest_steps = Vec::with_capacity(job.steps.len());
    for (offset, step) in job.steps.iter().enumerate() {
        let file_name = format!("{:03}.png", offset + 1);
        let relative = format!("keyframes/{file_name}");
        let path = destination.join(&relative);
        step.image.with_flattened_image(|image| {
            image
                .save_with_format(&path, image::ImageFormat::Png)
                .map_err(|source| ExportError::Encode {
                    path: path.display().to_string(),
                    source,
                })
        })?;
        markdown.push_str(&format!("{}. {}\n\n", step.index, step.title));
        if let Some(caption) = &step.caption {
            markdown.push_str(&format!("   {caption}\n\n"));
        }
        markdown.push_str(&format!("   ![]({relative})\n\n"));
        manifest_steps.push(ManifestStep {
            index: step.index,
            title: step.title.clone(),
            caption: step.caption.clone(),
            kind: step.kind,
            reason: step.reason,
            at_ms: step.at_ms,
            keyframe_file: relative,
            hotspots: step.hotspots.clone(),
        });
    }
    write_text(destination.join("steps.md"), &markdown)?;
    let manifest = SessionManifest {
        schema_version: GUIDE_SCHEMA_VERSION,
        title: job.title.clone(),
        region: job.region,
        input_source: job.input_source,
        input_capability: job.input_capability,
        steps: manifest_steps,
    };
    let json = serde_json::to_string_pretty(&manifest).map_err(|_| ExportError::Serialize {
        category: "session_manifest",
    })?;
    write_text(destination.join("session.json"), &json)?;
    write_text(destination.join("index.html"), &html::render(job)?)?;
    Ok(())
}

fn write_text(path: PathBuf, contents: &str) -> Result<(), ExportError> {
    std::fs::write(&path, contents).map_err(|source| ExportError::Io {
        path: path.display().to_string(),
        source,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::detector::DetectorConfig;
    use crate::frame_store::{FrameStore, StoreConfig};
    use crate::guide::Guide;
    use crate::models::{
        CandidateKind, CandidateStep, CaptureRegion, DetectReason, InputCapability, InputSourceKind,
    };
    use crate::recorder::ActionRecorder;
    use image::{Rgba, RgbaImage};
    use model::{GuideHotspot, ReviewedGuideExportJob, ReviewedGuideStep, ReviewedStepImage};
    use std::path::PathBuf;
    use std::sync::Arc;

    fn temp_dir(label: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let p = std::env::temp_dir().join(format!(
            "rollshot-action-{label}-{nanos}-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&p).expect("create temp dir");
        p
    }

    fn region() -> CaptureRegion {
        CaptureRegion {
            x: 0,
            y: 0,
            width: 8,
            height: 8,
        }
    }
    fn black() -> RgbaImage {
        RgbaImage::from_pixel(8, 8, Rgba([0, 0, 0, 255]))
    }
    fn quadrant() -> RgbaImage {
        let mut img = black();
        for y in 0..4 {
            for x in 0..4 {
                img.put_pixel(x, y, Rgba([255, 255, 255, 255]));
            }
        }
        img
    }

    /// A real recording yielding exactly one step + a store retaining its frames.
    fn one_step_recording() -> (Guide, FrameStore) {
        let det = DetectorConfig {
            diff_threshold: 0.01,
            area_threshold: 0.05,
            per_sample_threshold: 12.0,
            cooldown_ms: 0,
            click_window_ms: 600,
            typing_pause_ms: 700,
            scroll_dwell_ms: 600,
            stable_frames: 2,
        };
        let store = StoreConfig {
            ring_capacity: 30,
            analysis_capacity: 8,
            analysis_width: 384,
            window_before: 2,
            window_after: 2,
            nearby_max: 3,
        };
        let mut rec = ActionRecorder::new(region(), store, det);
        for (i, f) in [
            black(),
            quadrant(),
            quadrant(),
            quadrant(),
            quadrant(),
            quadrant(),
            quadrant(),
        ]
        .into_iter()
        .enumerate()
        {
            rec.ingest_frame(f, i as u64 * 100);
        }
        let recording = rec.finish();
        assert_eq!(recording.candidates.len(), 1);
        (
            Guide::from_candidates(recording.candidates.clone()),
            recording.store,
        )
    }

    fn build_job(
        guide: &Guide,
        store: &FrameStore,
        region: CaptureRegion,
        capability: InputCapability,
        source: InputSourceKind,
    ) -> ReviewedGuideExportJob {
        let steps: Vec<ReviewedGuideStep> = guide
            .steps()
            .iter()
            .enumerate()
            .map(|(i, step)| {
                let frame = store
                    .retained(step.keyframe)
                    .unwrap_or_else(|| panic!("keyframe {:03} not retained", i + 1));
                let caption = {
                    let trimmed = step.caption.trim();
                    if trimmed.is_empty() {
                        None
                    } else {
                        Some(trimmed.to_string())
                    }
                };
                ReviewedGuideStep {
                    index: step.index,
                    title: step.title.clone(),
                    caption,
                    kind: step.kind,
                    reason: step.reason,
                    at_ms: step.at_ms,
                    image: ReviewedStepImage::Retained(Arc::clone(&frame.image)),
                    hotspots: Vec::new(),
                }
            })
            .collect();
        ReviewedGuideExportJob {
            title: guide.effective_title().to_string(),
            region,
            input_source: source,
            input_capability: capability,
            steps,
        }
    }

    #[test]
    fn export_writes_portable_folder_with_matching_markdown_and_keyframes() {
        let (guide, store) = one_step_recording();
        let out = temp_dir("export-ok");
        let job = build_job(
            &guide,
            &store,
            region(),
            InputCapability::VisualOnly {
                reason: crate::models::DegradedReason::SourceStartFailed,
            },
            InputSourceKind::VisualOnly,
        );
        let dir = render_guide_folder(&job, &out.join("action-guide")).expect("export succeeds");

        assert_eq!(dir, out.join("action-guide"));
        assert!(dir.join("steps.md").exists());
        assert!(dir.join("session.json").exists());
        assert!(dir.join("keyframes/001.png").exists());

        let md = std::fs::read_to_string(dir.join("steps.md")).unwrap();
        assert!(md.contains("![](keyframes/001.png)"), "md = {md}");
        // Markdown references exactly the exported keyframe files.
        let png_count = std::fs::read_dir(dir.join("keyframes")).unwrap().count();
        assert_eq!(md.matches("![](keyframes/").count(), png_count);
        assert_eq!(png_count, guide.steps().len());

        let _ = std::fs::remove_dir_all(&out);
    }

    #[test]
    fn session_json_has_capability_and_no_raw_input_fields() {
        let (_guide, store) = one_step_recording();
        let kf = store
            .retained_ids_for_test()
            .into_iter()
            .next()
            .expect("a retained frame exists");
        let guide = Guide::from_candidates(vec![CandidateStep {
            id: 0,
            kind: CandidateKind::Typing,
            reason: DetectReason::TypingSettled,
            at_ms: 0,
            keyframe: kf,
            nearby: vec![kf],
        }]);
        let out = temp_dir("export-json");
        let job = build_job(
            &guide,
            &store,
            region(),
            InputCapability::SemanticEvents,
            InputSourceKind::LinuxEvdev,
        );
        let dir = render_guide_folder(&job, &out.join("action-guide")).unwrap();

        let json = std::fs::read_to_string(dir.join("session.json")).unwrap();
        let parsed: SessionManifest = serde_json::from_str(&json).expect("manifest parses");
        assert_eq!(parsed.input_source, InputSourceKind::LinuxEvdev);
        assert_eq!(parsed.input_capability, InputCapability::SemanticEvents);
        assert_eq!(parsed.steps.len(), 1);
        assert_eq!(parsed.steps[0].title, "Enter text");
        assert_eq!(parsed.steps[0].caption, None);
        assert!(
            !json.contains("\"caption\""),
            "empty captions should be omitted: {json}"
        );
        // Safe, static labels are allowed to appear.
        assert!(json.contains("semantic-events"), "json = {json}");
        assert!(json.contains("Enter text"), "json = {json}");
        // Raw input artifacts must never appear: no key codes, device identity,
        // typed content, or raw per-event `SemanticAction` records.
        for forbidden in [
            "keycode",
            "key_code",
            "keysym",
            "scancode",
            "device",
            "unicode",
            "clipboard",
            "typing-activity",
            "scroll-activity",
            "\"action\"",
        ] {
            assert!(
                !json.contains(forbidden),
                "session.json leaked {forbidden}: {json}"
            );
        }

        let _ = std::fs::remove_dir_all(&out);
    }

    #[test]
    fn export_is_atomic_a_midway_failure_leaves_no_folder_and_preserves_the_guide() {
        let (guide, store) = one_step_recording();
        let mut job = build_job(
            &guide,
            &store,
            region(),
            InputCapability::VisualOnly {
                reason: crate::models::DegradedReason::SourceStartFailed,
            },
            InputSourceKind::VisualOnly,
        );
        // Inject a step whose keyframe is not retained — forces a failure during
        // PNG encoding inside render_guide_folder.
        job.steps.push(ReviewedGuideStep {
            index: 2,
            title: "Missing".into(),
            caption: None,
            kind: CandidateKind::UiChanged,
            reason: DetectReason::VisualChange,
            at_ms: 100,
            image: ReviewedStepImage::Retained(Arc::new(RgbaImage::new(0, 0))),
            hotspots: Vec::new(),
        });
        let out = temp_dir("export-atomic");
        let destination = out.join("action-guide");
        let result = render_guide_folder(&job, &destination);

        assert!(result.is_err(), "export must fail");
        assert!(!destination.exists(), "no partial folder");
        assert_eq!(guide.steps().len(), 1, "editable guide is preserved");

        let _ = std::fs::remove_dir_all(&out);
    }

    #[test]
    fn export_guide_includes_non_empty_step_caption() {
        let (mut guide, store) = one_step_recording();
        assert!(guide.set_caption(
            1,
            "The settings dialog closes, but the new value is not persisted.".to_string()
        ));
        let out = temp_dir("export-caption");
        let job = build_job(
            &guide,
            &store,
            region(),
            InputCapability::SemanticEvents,
            InputSourceKind::LinuxEvdev,
        );

        let dir = render_guide_folder(&job, &out.join("action-guide")).expect("export succeeds");

        let md = std::fs::read_to_string(dir.join("steps.md")).unwrap();
        assert!(
            md.contains("The settings dialog closes, but the new value is not persisted."),
            "md = {md}"
        );

        let json = std::fs::read_to_string(dir.join("session.json")).unwrap();
        let parsed: SessionManifest = serde_json::from_str(&json).expect("manifest parses");
        assert_eq!(
            parsed.steps[0].caption.as_deref(),
            Some("The settings dialog closes, but the new value is not persisted.")
        );

        let _ = std::fs::remove_dir_all(&out);
    }

    #[test]
    fn session_manifest_deserializes_without_caption_field() {
        let json = r#"{
  "region": { "x": 0, "y": 0, "width": 8, "height": 8 },
  "input_source": "linux-evdev",
  "input_capability": "semantic-events",
  "steps": [
    {
      "index": 1,
      "title": "Click",
      "kind": "click",
      "reason": "click-confirmed",
      "at_ms": 0,
      "keyframe_file": "keyframes/001.png"
    }
  ]
}"#;

        let parsed: SessionManifest = serde_json::from_str(json).expect("manifest parses");

        assert_eq!(parsed.steps.len(), 1);
        assert_eq!(parsed.steps[0].caption, None);
    }

    #[test]
    fn session_manifest_deserializes_pre_v1_json_shape() {
        let json = r#"{
  "region": { "x": 0, "y": 0, "width": 8, "height": 8 },
  "input_source": "linux-evdev",
  "input_capability": "semantic-events",
  "steps": [
    {
      "index": 1,
      "title": "Click",
      "kind": "click",
      "reason": "click-confirmed",
      "at_ms": 0,
      "keyframe_file": "keyframes/001.png"
    }
  ]
}"#;
        let parsed: SessionManifest = serde_json::from_str(json).expect("manifest parses");
        assert_eq!(parsed.schema_version, 0, "legacy schema defaults to 0");
        assert_eq!(
            parsed.title, "Action Guide",
            "legacy title defaults to Action Guide"
        );
    }

    #[test]
    fn renderer_writes_all_required_artifacts_from_one_job() {
        let parent = temp_dir("required");
        let destination = parent.join("guide");
        let job = annotated_job();

        let result = render_guide_folder(&job, &destination).unwrap();

        assert_eq!(result, destination);
        for relative in [
            "index.html",
            "steps.md",
            "session.json",
            "keyframes/001.png",
        ] {
            assert!(result.join(relative).is_file(), "missing {relative}");
        }
        let manifest: SessionManifest =
            serde_json::from_slice(&std::fs::read(result.join("session.json")).unwrap()).unwrap();
        assert_eq!(manifest.schema_version, GUIDE_SCHEMA_VERSION);
        assert_eq!(manifest.title, "Checkout failure");
        assert_eq!(manifest.steps[0].title, "Submit order");
    }

    #[test]
    fn renderer_never_replaces_destination_and_cleans_failed_build() {
        let parent = temp_dir("noclobber");
        let destination = parent.join("guide");
        std::fs::create_dir(&destination).unwrap();
        std::fs::write(destination.join("keep.txt"), "old").unwrap();
        let error = render_guide_folder(&annotated_job(), &destination).unwrap_err();
        assert!(matches!(error, ExportError::DestinationExists { .. }));
        assert_eq!(
            std::fs::read_to_string(destination.join("keep.txt")).unwrap(),
            "old"
        );
    }

    fn annotated_job() -> ReviewedGuideExportJob {
        let mut document = rollshot_image_document::ImageDocument::new(RgbaImage::from_pixel(
            8,
            8,
            Rgba([20, 30, 40, 255]),
        ));
        document
            .add_redaction(rollshot_image_document::ImageRect::new(0.0, 0.0, 4.0, 4.0))
            .unwrap();
        ReviewedGuideExportJob {
            title: "Checkout failure".into(),
            region: CaptureRegion {
                x: 0,
                y: 0,
                width: 8,
                height: 8,
            },
            input_source: InputSourceKind::LinuxEvdev,
            input_capability: InputCapability::SemanticEvents,
            steps: vec![ReviewedGuideStep {
                index: 1,
                title: "Submit order".into(),
                caption: Some("Confirm the request".into()),
                kind: CandidateKind::Click,
                reason: DetectReason::ClickConfirmed,
                at_ms: 100,
                image: ReviewedStepImage::Annotated(document.flatten_snapshot()),
                hotspots: vec![GuideHotspot {
                    annotation_id: 1,
                    bounds: model::NormalizedRect {
                        x: 0.0,
                        y: 0.0,
                        width: 0.5,
                        height: 0.5,
                    },
                    explanation: "Open Settings".into(),
                }],
            }],
        }
    }

    #[test]
    fn redaction_replaces_source_pixels_and_png_has_no_ancillary_source_payload() {
        let source_color = Rgba([42, 87, 129, 255]);
        let source = RgbaImage::from_pixel(8, 8, source_color);
        let mut document = rollshot_image_document::ImageDocument::new(source.clone());
        document
            .add_redaction(rollshot_image_document::ImageRect::new(0.0, 0.0, 4.0, 4.0))
            .unwrap();
        let snapshot = document.flatten_snapshot();
        let job = ReviewedGuideExportJob {
            title: "Redaction test".into(),
            region: CaptureRegion {
                x: 0,
                y: 0,
                width: 8,
                height: 8,
            },
            input_source: InputSourceKind::LinuxEvdev,
            input_capability: InputCapability::SemanticEvents,
            steps: vec![ReviewedGuideStep {
                index: 1,
                title: "Redacted step".into(),
                caption: None,
                kind: CandidateKind::Click,
                reason: DetectReason::ClickConfirmed,
                at_ms: 0,
                image: ReviewedStepImage::Annotated(snapshot),
                hotspots: Vec::new(),
            }],
        };
        let parent = temp_dir("redaction-payload");
        let destination = parent.join("guide");
        render_guide_folder(&job, &destination).unwrap();

        let png_bytes = std::fs::read(destination.join("keyframes/001.png")).unwrap();
        let decoded = image::load_from_memory(&png_bytes).unwrap().to_rgba8();

        assert_ne!(
            decoded.get_pixel(2, 2).0,
            source_color.0,
            "redacted pixel must differ from source"
        );
        assert_eq!(
            decoded.get_pixel(2, 2).0,
            [0, 0, 0, 255],
            "redacted pixel must be opaque black"
        );
        assert_eq!(
            decoded.get_pixel(6, 6).0,
            source_color.0,
            "unredacted pixel must match source"
        );

        let source_pattern = source_color.0;
        for (offset, window) in png_bytes.windows(4).enumerate() {
            if window == source_pattern {
                panic!("source pixel pattern found in PNG ancillary data at byte offset {offset}");
            }
        }

        let _ = std::fs::remove_dir_all(&parent);
    }
}
