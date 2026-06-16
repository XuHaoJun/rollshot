use std::path::{Path, PathBuf};

use rollshot_action::{
    export_guide, CaptureRegion, Guide, InputCapability, InputSourceKind, Recording,
};

pub fn export_recording(
    recording: Recording,
    region: CaptureRegion,
    capability: InputCapability,
    source_kind: InputSourceKind,
    out_dir: &Path,
) -> Result<PathBuf, String> {
    let Recording { candidates, store } = recording;
    let guide = Guide::from_candidates(candidates);
    export_guide(&guide, &store, region, capability, source_kind, out_dir)
        .map_err(|e| format!("export failed: {e}"))
}

pub fn default_out_dir(now_ms: u64) -> PathBuf {
    let base = dirs::picture_dir().unwrap_or_else(std::env::temp_dir);
    base.join(format!("rollshot-action-{now_ms}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{Rgba, RgbaImage};
    use rollshot_action::{ActionRecorder, CaptureRegion, DetectorConfig, StoreConfig};

    fn black_32() -> RgbaImage {
        RgbaImage::from_pixel(32, 32, Rgba([0, 0, 0, 255]))
    }

    fn white_quadrant_32() -> RgbaImage {
        let mut img = black_32();
        for y in 0..16 {
            for x in 0..16 {
                img.put_pixel(x, y, Rgba([255, 255, 255, 255]));
            }
        }
        img
    }

    #[test]
    fn export_recording_writes_steps_md() {
        let region = CaptureRegion {
            x: 0,
            y: 0,
            width: 32,
            height: 32,
        };
        let det = DetectorConfig {
            diff_threshold: 0.01,
            area_threshold: 0.05,
            cooldown_ms: 0,
            ..DetectorConfig::default()
        };
        let mut rec = ActionRecorder::new(region, StoreConfig::default(), det);
        rec.ingest_frame(black_32(), 0);
        for i in 1..=6 {
            rec.ingest_frame(white_quadrant_32(), i * 100);
        }
        let recording = rec.finish();
        assert!(
            !recording.candidates.is_empty(),
            "detector should find at least one step"
        );
        let tmp = std::env::temp_dir().join("rollshot-action-export-test");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        let out = export_recording(
            recording,
            region,
            rollshot_action::InputCapability::SemanticEvents,
            rollshot_action::InputSourceKind::LinuxEvdev,
            &tmp,
        )
        .unwrap();
        assert!(out.join("steps.md").exists());
        assert!(out.join("session.json").exists());
        let _ = std::fs::remove_dir_all(&tmp);
    }
}
