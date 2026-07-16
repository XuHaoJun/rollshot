//! Portable guide export. Builds `action-guide/{steps.md, session.json,
//! keyframes/*.png}` in a temporary sibling directory and renames it into place
//! only after every file is written. Any failure rolls back the temp dir, so
//! there is never a partial export and the editable session is preserved.
//! `session.json` serializes only step metadata + capability — never raw input.

pub mod model;

use std::path::{Path, PathBuf};

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

fn non_empty_caption(caption: &str) -> Option<&str> {
    let caption = caption.trim();
    (!caption.is_empty()).then_some(caption)
}

/// Export `guide` into `out_dir/action-guide/`. Returns the created directory.
#[deprecated(note = "build a ReviewedGuideExportJob and call render_guide_folder")]
pub fn export_guide(
    guide: &Guide,
    store: &FrameStore,
    region: CaptureRegion,
    capability: InputCapability,
    source: InputSourceKind,
    out_dir: &Path,
) -> Result<PathBuf, ExportError> {
    if guide.is_empty() {
        return Err(ExportError::Empty);
    }
    let final_dir = out_dir.join("action-guide");
    let tmp_dir = out_dir.join(".action-guide.tmp");

    if tmp_dir.exists() {
        std::fs::remove_dir_all(&tmp_dir).map_err(|source| ExportError::Io {
            path: tmp_dir.display().to_string(),
            source,
        })?;
    }

    // Build everything in the temp dir, then swap it into place. On ANY failure
    // — during build OR during the swap — remove the temp dir so no partial
    // `.action-guide.tmp` artifact is left behind and the editable session is
    // preserved.
    if let Err(err) = build(guide, store, region, capability, source, &tmp_dir)
        .and_then(|()| swap_into_place(&tmp_dir, &final_dir))
    {
        let _ = std::fs::remove_dir_all(&tmp_dir);
        tracing::debug!(target: TARGET_EXPORT, "export failed; temp dir rolled back");
        return Err(err);
    }
    tracing::info!(target: TARGET_EXPORT, steps = guide.steps().len(), "export complete");
    Ok(final_dir)
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

/// Replace `final_dir` with the freshly-built `tmp_dir`: remove the previous
/// export (if any), then rename the temp dir into place.
fn swap_into_place(tmp_dir: &Path, final_dir: &Path) -> Result<(), ExportError> {
    if final_dir.exists() {
        std::fs::remove_dir_all(final_dir).map_err(|source| ExportError::Io {
            path: final_dir.display().to_string(),
            source,
        })?;
    }
    std::fs::rename(tmp_dir, final_dir).map_err(|source| ExportError::Io {
        path: final_dir.display().to_string(),
        source,
    })
}

fn build(
    guide: &Guide,
    store: &FrameStore,
    region: CaptureRegion,
    capability: InputCapability,
    source: InputSourceKind,
    tmp: &Path,
) -> Result<(), ExportError> {
    let keyframes = tmp.join("keyframes");
    std::fs::create_dir_all(&keyframes).map_err(|source| ExportError::Io {
        path: keyframes.display().to_string(),
        source,
    })?;

    let mut md = String::from("# Action Guide\n\n");
    let mut steps = Vec::new();

    for (i, step) in guide.steps().iter().enumerate() {
        let n = i + 1;
        let file_name = format!("{n:03}.png");
        let rel = format!("keyframes/{file_name}");
        let frame = store
            .retained(step.keyframe)
            .ok_or_else(|| ExportError::Io {
                path: rel.clone(),
                source: std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    "keyframe pixels not retained",
                ),
            })?;
        let png_path = keyframes.join(&file_name);
        frame
            .image
            .save_with_format(&png_path, image::ImageFormat::Png)
            .map_err(|source| ExportError::Encode {
                path: png_path.display().to_string(),
                source,
            })?;
        let caption = non_empty_caption(&step.caption);
        md.push_str(&format!("{n}. {}\n\n", step.title));
        if let Some(caption) = caption {
            md.push_str(&format!("   {caption}\n\n"));
        }
        md.push_str(&format!("   ![]({rel})\n\n"));
        steps.push(ManifestStep {
            index: step.index,
            title: step.title.clone(),
            caption: caption.map(str::to_string),
            kind: step.kind,
            reason: step.reason,
            at_ms: step.at_ms,
            keyframe_file: rel,
            hotspots: Vec::new(),
        });
    }

    std::fs::write(tmp.join("steps.md"), md).map_err(|source| ExportError::Io {
        path: tmp.join("steps.md").display().to_string(),
        source,
    })?;

    let manifest = SessionManifest {
        schema_version: 0,
        title: crate::guide::DEFAULT_GUIDE_TITLE.to_string(),
        region,
        input_source: source,
        input_capability: capability,
        steps,
    };
    let json = serde_json::to_string_pretty(&manifest).map_err(|e| ExportError::Io {
        path: "session.json".to_string(),
        source: std::io::Error::other(e.to_string()),
    })?;
    std::fs::write(tmp.join("session.json"), json).map_err(|source| ExportError::Io {
        path: tmp.join("session.json").display().to_string(),
        source,
    })?;
    Ok(())
}

#[cfg(test)]
#[allow(deprecated)]
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

    #[test]
    fn export_writes_portable_folder_with_matching_markdown_and_keyframes() {
        let (guide, store) = one_step_recording();
        let out = temp_dir("export-ok");
        let dir = export_guide(
            &guide,
            &store,
            region(),
            InputCapability::VisualOnly {
                reason: crate::models::DegradedReason::SourceStartFailed,
            },
            InputSourceKind::VisualOnly,
            &out,
        )
        .expect("export succeeds");

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
        // Exercise the exact cases that make a naive substring scan brittle: a
        // Typing step (default title "Enter text") exported under SemanticEvents
        // capability ("semantic-events"). These safe, static labels MUST be
        // tolerated, while raw input artifacts MUST never appear.
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
        let dir = export_guide(
            &guide,
            &store,
            region(),
            InputCapability::SemanticEvents,
            InputSourceKind::LinuxEvdev,
            &out,
        )
        .unwrap();

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
        let (_guide, store) = one_step_recording();
        let kf = store
            .retained_ids_for_test()
            .into_iter()
            .next()
            .expect("a retained frame exists");
        // Step 1 is exportable; step 2's keyframe is not retained -> fails mid-export.
        let guide = Guide::from_candidates(vec![
            CandidateStep {
                id: 0,
                kind: CandidateKind::Click,
                reason: DetectReason::ClickConfirmed,
                at_ms: 0,
                keyframe: kf,
                nearby: vec![kf],
            },
            CandidateStep {
                id: 1,
                kind: CandidateKind::UiChanged,
                reason: DetectReason::VisualChange,
                at_ms: 100,
                keyframe: 999_999, // not retained -> injected failure
                nearby: vec![999_999],
            },
        ]);
        let out = temp_dir("export-atomic");
        let result = export_guide(
            &guide,
            &store,
            region(),
            InputCapability::VisualOnly {
                reason: crate::models::DegradedReason::SourceStartFailed,
            },
            InputSourceKind::VisualOnly,
            &out,
        );

        assert!(result.is_err(), "export must fail");
        assert!(!out.join("action-guide").exists(), "no partial folder");
        assert!(
            !out.join(".action-guide.tmp").exists(),
            "temp dir rolled back"
        );
        assert_eq!(guide.steps().len(), 2, "editable guide is preserved");

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

        let dir = export_guide(
            &guide,
            &store,
            region(),
            InputCapability::SemanticEvents,
            InputSourceKind::LinuxEvdev,
            &out,
        )
        .expect("export succeeds");

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
}
