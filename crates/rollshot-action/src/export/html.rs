use crate::error::ExportError;
use crate::export::model::{GuideHotspot, ReviewedGuideExportJob, GUIDE_SCHEMA_VERSION};

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct ViewerGuide {
    schema_version: u32,
    title: String,
    steps: Vec<ViewerStep>,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct ViewerStep {
    index: usize,
    title: String,
    caption: Option<String>,
    keyframe_file: String,
    image_width: u32,
    image_height: u32,
    hotspots: Vec<GuideHotspot>,
}

impl From<&ReviewedGuideExportJob> for ViewerGuide {
    fn from(job: &ReviewedGuideExportJob) -> Self {
        Self {
            schema_version: GUIDE_SCHEMA_VERSION,
            title: job.title.clone(),
            steps: job
                .steps
                .iter()
                .enumerate()
                .map(|(offset, step)| {
                    let (image_width, image_height) = step.image.dimensions();
                    ViewerStep {
                        index: step.index,
                        title: step.title.clone(),
                        caption: step.caption.clone(),
                        keyframe_file: format!("keyframes/{:03}.png", offset + 1),
                        image_width,
                        image_height,
                        hotspots: step.hotspots.clone(),
                    }
                })
                .collect(),
        }
    }
}

fn escape_script_data(json: &str) -> String {
    json.replace('&', "\\u0026")
        .replace('<', "\\u003c")
        .replace('>', "\\u003e")
        .replace('\u{2028}', "\\u2028")
        .replace('\u{2029}', "\\u2029")
}

pub(crate) fn render(job: &ReviewedGuideExportJob) -> Result<String, ExportError> {
    let data = escape_script_data(&serde_json::to_string(&ViewerGuide::from(job)).map_err(
        |_| ExportError::Serialize {
            category: "viewer_data",
        },
    )?);
    let template = include_str!("viewer.html");
    let marker = "__ROLLSHOT_GUIDE_DATA__";
    if template.matches(marker).count() != 1 {
        return Err(ExportError::Template {
            category: "data_marker",
        });
    }
    Ok(template.replace(marker, &data))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::export::model::{
        GuideHotspot, NormalizedRect, ReviewedGuideExportJob, ReviewedGuideStep, ReviewedStepImage,
    };
    use crate::models::{
        CandidateKind, CaptureRegion, DetectReason, InputCapability, InputSourceKind,
    };
    use image::{Rgba, RgbaImage};
    use rollshot_image_document::ImageDocument;

    fn annotated_job() -> ReviewedGuideExportJob {
        let mut document = ImageDocument::new(RgbaImage::from_pixel(8, 8, Rgba([20, 30, 40, 255])));
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
                    bounds: NormalizedRect {
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
    fn embedded_json_cannot_close_its_script_element() {
        let mut job = annotated_job();
        job.title = "</script><script>globalThis.pwned=true</script>".into();
        let html = render(&job).unwrap();
        assert!(
            !html.contains("</script><script>globalThis.pwned"),
            "script injection not prevented"
        );
        assert!(
            html.contains("\\u003c/script\\u003e"),
            "escaped < not found"
        );
    }

    #[test]
    fn render_produces_valid_html_with_marker_replaced() {
        let job = annotated_job();
        let html = render(&job).unwrap();
        assert!(
            !html.contains("__ROLLSHOT_GUIDE_DATA__"),
            "marker not replaced"
        );
        assert!(html.contains("Checkout failure"), "title missing");
        assert!(html.contains("Submit order"), "step title missing");
    }
}
