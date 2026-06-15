//! Portable guide export. Builds `action-guide/{steps.md, session.json,
//! keyframes/*.png}` in a temporary sibling directory and renames it into place
//! only after every file is written. Any failure rolls back the temp dir, so
//! there is never a partial export and the editable session is preserved.
//! `session.json` serializes only step metadata + capability — never raw input.

use std::path::{Path, PathBuf};

use crate::diagnostics::TARGET_EXPORT;
use crate::error::ExportError;
use crate::frame_store::FrameStore;
use crate::guide::Guide;
use crate::models::{
    CandidateKind, CaptureRegion, DetectReason, InputCapability, InputSourceKind, Millis,
};

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SessionManifest {
    pub region: CaptureRegion,
    pub input_source: InputSourceKind,
    pub input_capability: InputCapability,
    pub steps: Vec<ManifestStep>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ManifestStep {
    pub index: usize,
    pub title: String,
    pub kind: CandidateKind,
    pub reason: DetectReason,
    pub at_ms: Millis,
    pub keyframe_file: String,
}

/// Export `guide` into `out_dir/action-guide/`. Returns the created directory.
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

    // Build everything in the temp dir; roll back the whole dir on any error.
    if let Err(err) = build(guide, store, region, capability, source, &tmp_dir) {
        let _ = std::fs::remove_dir_all(&tmp_dir);
        tracing::debug!(target: TARGET_EXPORT, "export failed; temp dir rolled back");
        return Err(err);
    }

    if final_dir.exists() {
        std::fs::remove_dir_all(&final_dir).map_err(|source| ExportError::Io {
            path: final_dir.display().to_string(),
            source,
        })?;
    }
    std::fs::rename(&tmp_dir, &final_dir).map_err(|source| ExportError::Io {
        path: final_dir.display().to_string(),
        source,
    })?;
    tracing::info!(target: TARGET_EXPORT, steps = guide.steps().len(), "export complete");
    Ok(final_dir)
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
        md.push_str(&format!("{n}. {}\n\n   ![]({rel})\n\n", step.title));
        steps.push(ManifestStep {
            index: step.index,
            title: step.title.clone(),
            kind: step.kind,
            reason: step.reason,
            at_ms: step.at_ms,
            keyframe_file: rel,
        });
    }

    std::fs::write(tmp.join("steps.md"), md).map_err(|source| ExportError::Io {
        path: tmp.join("steps.md").display().to_string(),
        source,
    })?;

    let manifest = SessionManifest {
        region,
        input_source: source,
        input_capability: capability,
        steps,
    };
    let json = serde_json::to_string_pretty(&manifest).map_err(|e| ExportError::Io {
        path: "session.json".to_string(),
        source: std::io::Error::new(std::io::ErrorKind::Other, e.to_string()),
    })?;
    std::fs::write(tmp.join("session.json"), json).map_err(|source| ExportError::Io {
        path: tmp.join("session.json").display().to_string(),
        source,
    })?;
    Ok(())
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
        let (guide, store) = one_step_recording();
        let out = temp_dir("export-json");
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
        .unwrap();

        let json = std::fs::read_to_string(dir.join("session.json")).unwrap();
        let parsed: SessionManifest = serde_json::from_str(&json).expect("manifest parses");
        assert_eq!(parsed.input_source, InputSourceKind::VisualOnly);
        assert_eq!(parsed.steps.len(), 1);
        assert!(json.contains("visual-only"));
        for forbidden in [
            "semantic",
            "events",
            "text",
            "keycode",
            "device",
            "typing-activity",
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
}
