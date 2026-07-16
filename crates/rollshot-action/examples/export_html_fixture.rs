use image::{Rgba, RgbaImage};
use rollshot_action::{
    CandidateKind, CaptureRegion, DetectReason, GuideHotspot, InputCapability, InputSourceKind,
    NormalizedRect, ReviewedGuideExportJob, ReviewedGuideStep, ReviewedStepImage,
};
use std::{path::PathBuf, sync::Arc};

fn fixture_job() -> ReviewedGuideExportJob {
    let titles = [
        "Open Settings",
        "Submit </script><script>globalThis.pwned=true</script>",
        "Verify result",
        "Finish",
    ];
    let colors = [
        [40, 90, 180, 255],
        [50, 150, 90, 255],
        [180, 100, 40, 255],
        [120, 70, 170, 255],
    ];
    let steps = titles
        .into_iter()
        .zip(colors)
        .enumerate()
        .map(|(offset, (title, color))| {
            let hotspots = if offset == 0 {
                vec![
                    GuideHotspot {
                        annotation_id: 10,
                        bounds: NormalizedRect {
                            x: 0.1,
                            y: 0.1,
                            width: 0.2,
                            height: 0.2,
                        },
                        explanation: "Open Settings".into(),
                    },
                    GuideHotspot {
                        annotation_id: 11,
                        bounds: NormalizedRect {
                            x: 0.6,
                            y: 0.5,
                            width: 0.2,
                            height: 0.2,
                        },
                        explanation: "Choose Privacy".into(),
                    },
                ]
            } else {
                Vec::new()
            };
            ReviewedGuideStep {
                index: offset + 1,
                title: title.into(),
                caption: Some(format!("Caption for step {}", offset + 1)),
                kind: CandidateKind::Click,
                reason: DetectReason::VisualChange,
                at_ms: (offset as u64 + 1) * 100,
                image: ReviewedStepImage::Retained(Arc::new(RgbaImage::from_pixel(
                    320,
                    180,
                    Rgba(color),
                ))),
                hotspots,
            }
        })
        .collect();
    ReviewedGuideExportJob {
        title: "Checkout failure".into(),
        region: CaptureRegion {
            x: 0,
            y: 0,
            width: 320,
            height: 180,
        },
        input_source: InputSourceKind::LinuxEvdev,
        input_capability: InputCapability::SemanticEvents,
        steps,
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let destination = std::env::args_os()
        .nth(1)
        .ok_or("destination argument required")?;
    let destination = PathBuf::from(destination);
    if destination.exists() {
        std::fs::remove_dir_all(&destination)?;
    }
    if let Some(parent) = destination.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let job = fixture_job();
    rollshot_action::render_guide_folder(&job, &destination)?;
    Ok(())
}
