use std::sync::Arc;

use image::RgbaImage;
use rollshot_image_document::FlattenSnapshot;

use crate::error::ExportError;
use crate::models::{
    CandidateKind, CaptureRegion, DetectReason, InputCapability, InputSourceKind, Millis,
};

pub const GUIDE_SCHEMA_VERSION: u32 = 1;

pub struct ReviewedGuideExportJob {
    pub title: String,
    pub region: CaptureRegion,
    pub input_source: InputSourceKind,
    pub input_capability: InputCapability,
    pub steps: Vec<ReviewedGuideStep>,
}

pub struct ReviewedGuideStep {
    pub index: usize,
    pub title: String,
    pub caption: Option<String>,
    pub kind: CandidateKind,
    pub reason: DetectReason,
    pub at_ms: Millis,
    pub image: ReviewedStepImage,
    pub hotspots: Vec<GuideHotspot>,
}

pub enum ReviewedStepImage {
    Retained(Arc<RgbaImage>),
    Annotated(FlattenSnapshot),
}

impl ReviewedStepImage {
    pub fn dimensions(&self) -> (u32, u32) {
        match self {
            Self::Retained(image) => image.dimensions(),
            Self::Annotated(snapshot) => snapshot.dimensions(),
        }
    }

    pub(crate) fn with_flattened_image<T>(
        &self,
        use_image: impl FnOnce(&RgbaImage) -> Result<T, ExportError>,
    ) -> Result<T, ExportError> {
        match self {
            Self::Retained(image) => use_image(image),
            Self::Annotated(snapshot) => {
                let flattened = snapshot.flatten();
                use_image(&flattened)
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GuideHotspot {
    pub annotation_id: u64,
    pub bounds: NormalizedRect,
    pub explanation: String,
}

#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NormalizedRect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl NormalizedRect {
    fn is_finite_positive(&self) -> bool {
        self.x.is_finite()
            && self.y.is_finite()
            && self.width.is_finite()
            && self.height.is_finite()
            && self.width > 0.0
            && self.height > 0.0
    }

    fn is_within_unit_square(&self) -> bool {
        self.x >= 0.0
            && self.y >= 0.0
            && (self.x + self.width) <= 1.0
            && (self.y + self.height) <= 1.0
    }
}

impl ReviewedGuideExportJob {
    pub fn validate(&self) -> Result<(), ExportError> {
        if self.title.trim().is_empty() {
            return Err(ExportError::InvalidHotspot {
                step: 0,
                category: "empty_title",
            });
        }
        if self.steps.is_empty() {
            return Err(ExportError::Empty);
        }
        for (i, step) in self.steps.iter().enumerate() {
            let expected = i + 1;
            if step.index != expected {
                return Err(ExportError::InvalidHotspot {
                    step: step.index,
                    category: "invalid_index",
                });
            }
            for hotspot in &step.hotspots {
                let explanation = hotspot.explanation.trim();
                if explanation.is_empty() {
                    return Err(ExportError::InvalidHotspot {
                        step: step.index,
                        category: "empty_text",
                    });
                }
                if !hotspot.bounds.is_finite_positive() {
                    return Err(ExportError::InvalidHotspot {
                        step: step.index,
                        category: "non_finite",
                    });
                }
                if !hotspot.bounds.is_within_unit_square() {
                    return Err(ExportError::InvalidHotspot {
                        step: step.index,
                        category: "outside_image",
                    });
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn one_step_job() -> ReviewedGuideExportJob {
        ReviewedGuideExportJob {
            title: "Checkout failure".into(),
            region: CaptureRegion {
                x: 0,
                y: 0,
                width: 8,
                height: 8,
            },
            input_source: InputSourceKind::VisualOnly,
            input_capability: InputCapability::VisualOnly {
                reason: crate::models::DegradedReason::SourceStartFailed,
            },
            steps: vec![ReviewedGuideStep {
                index: 1,
                title: "Submit order".into(),
                caption: Some("Confirm the request".into()),
                kind: CandidateKind::Click,
                reason: DetectReason::VisualChange,
                at_ms: 100,
                image: ReviewedStepImage::Retained(Arc::new(RgbaImage::new(8, 8))),
                hotspots: vec![GuideHotspot {
                    annotation_id: 1,
                    bounds: NormalizedRect {
                        x: 0.1,
                        y: 0.1,
                        width: 0.2,
                        height: 0.2,
                    },
                    explanation: "Open settings".into(),
                }],
            }],
        }
    }

    #[test]
    fn validation_rejects_non_finite_or_outside_hotspots() {
        let mut job = one_step_job();
        job.steps[0].hotspots.push(GuideHotspot {
            annotation_id: 7,
            bounds: NormalizedRect {
                x: f32::NAN,
                y: 0.0,
                width: 0.2,
                height: 0.2,
            },
            explanation: "Open settings".into(),
        });
        assert!(matches!(
            job.validate(),
            Err(ExportError::InvalidHotspot { step: 1, .. })
        ));

        job.steps[0].hotspots.pop();
        job.steps[0].hotspots[0].bounds = NormalizedRect {
            x: 1.1,
            y: 0.0,
            width: 0.2,
            height: 0.2,
        };
        assert!(job.validate().is_err());

        job.steps[0].hotspots[0].bounds = NormalizedRect {
            x: 0.9,
            y: 0.0,
            width: 0.2,
            height: 0.2,
        };
        assert!(job.validate().is_err());
    }

    #[test]
    fn validation_rejects_empty_explanation_and_accepts_valid_job() {
        let mut job = one_step_job();
        job.steps[0].hotspots[0].explanation = "  ".into();
        assert!(matches!(
            job.validate(),
            Err(ExportError::InvalidHotspot { step: 1, .. })
        ));
        job.steps[0].hotspots[0].explanation = "Open settings".into();
        assert!(job.validate().is_ok());
    }
}
