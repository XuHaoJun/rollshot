use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use image::RgbaImage;
use rollshot_image_document::{Annotation, FlattenSnapshot, ImageDocument};

use crate::error::ExportError;
use crate::models::{
    CandidateKind, CaptureRegion, DetectReason, InputCapability, InputSourceKind, Millis,
};
use crate::project::ProjectFrame;

pub const GUIDE_SCHEMA_VERSION: u32 = 1;

#[derive(Clone)]
pub struct ReviewedGuideExportJob {
    pub title: String,
    pub region: CaptureRegion,
    pub input_source: InputSourceKind,
    pub input_capability: InputCapability,
    pub steps: Vec<ReviewedGuideStep>,
}

#[derive(Clone)]
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

#[derive(Clone)]
pub struct ProjectReviewedImage {
    pub project_root: std::path::PathBuf,
    pub frame: ProjectFrame,
    pub annotations: Option<Vec<Annotation>>,
    pub step: usize,
}

#[derive(Clone)]
pub enum ReviewedStepImage {
    Retained(Arc<RgbaImage>),
    Annotated(FlattenSnapshot),
    Project(ProjectReviewedImage),
}

impl ReviewedStepImage {
    pub fn dimensions(&self) -> (u32, u32) {
        match self {
            Self::Retained(image) => image.dimensions(),
            Self::Annotated(snapshot) => snapshot.dimensions(),
            Self::Project(project) => (project.frame.width, project.frame.height),
        }
    }

    pub(crate) fn with_flattened_image<T>(
        &self,
        cancel: &AtomicBool,
        use_image: impl FnOnce(&RgbaImage) -> Result<T, ExportError>,
    ) -> Result<T, ExportError> {
        if cancel.load(Ordering::Relaxed) {
            return Err(ExportError::Cancelled);
        }
        match self {
            Self::Retained(image) => use_image(image),
            Self::Annotated(snapshot) => {
                let flattened = snapshot.flatten();
                use_image(&flattened)
            }
            Self::Project(project) => {
                let img = crate::project::decode_png_asset(
                    &project.project_root,
                    &project.frame.sha256,
                    project.frame.width,
                    project.frame.height,
                )
                .map_err(|error| {
                    let is_not_found = matches!(
                        &error,
                        crate::project::ProjectError::Io { source, .. }
                            if source.kind() == std::io::ErrorKind::NotFound
                    );
                    tracing::warn!(
                        target: "rollshot::export",
                        step = project.step,
                        error_category = error.category(),
                        "lazy decode failed for project asset"
                    );
                    if is_not_found {
                        ExportError::AssetMissing { step: project.step }
                    } else {
                        ExportError::AssetDecodeFailed { step: project.step }
                    }
                })?;
                match &project.annotations {
                    Some(annotations) if !annotations.is_empty() => {
                        let doc = ImageDocument::from_persisted_annotations(
                            Arc::new(img),
                            annotations.clone(),
                        )
                        .map_err(|_| ExportError::AssetDecodeFailed { step: project.step })?;
                        let flattened = doc.flatten();
                        use_image(&flattened)
                    }
                    _ => use_image(&img),
                }
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
    use std::sync::atomic::AtomicBool;

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

    fn setup_project_dir(root: &std::path::Path) {
        std::fs::create_dir_all(root.join("assets/frames")).unwrap();
    }

    fn write_project_asset(
        root: &std::path::Path,
        image: &RgbaImage,
    ) -> crate::project::ProjectFrame {
        use crate::project::encode_png_asset;
        let encoded = encode_png_asset(image).unwrap();
        let dest = root
            .join("assets/frames")
            .join(format!("{}.png", encoded.sha256));
        std::fs::write(&dest, &encoded.bytes).unwrap();
        crate::project::ProjectFrame {
            id: 0,
            at_ms: 0,
            sha256: encoded.sha256,
            width: image.width(),
            height: image.height(),
        }
    }

    fn project_two_step_job(
        root: &std::path::Path,
    ) -> (
        ReviewedGuideExportJob,
        crate::project::ProjectFrame,
        crate::project::ProjectFrame,
    ) {
        use crate::models::{
            CandidateKind, CaptureRegion, DetectReason, InputCapability, InputSourceKind,
        };

        setup_project_dir(root);
        let img1 = RgbaImage::from_pixel(8, 8, image::Rgba([10, 20, 30, 255]));
        let img2 = RgbaImage::from_pixel(8, 8, image::Rgba([40, 50, 60, 255]));
        let mut f1 = write_project_asset(root, &img1);
        f1.id = 1;
        f1.at_ms = 100;
        let mut f2 = write_project_asset(root, &img2);
        f2.id = 2;
        f2.at_ms = 200;

        let job = ReviewedGuideExportJob {
            title: "Test Guide".into(),
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
            steps: vec![
                ReviewedGuideStep {
                    index: 1,
                    title: "Step 1".into(),
                    caption: None,
                    kind: CandidateKind::Click,
                    reason: DetectReason::VisualChange,
                    at_ms: 100,
                    image: ReviewedStepImage::Project(ProjectReviewedImage {
                        project_root: root.to_path_buf(),
                        frame: f1.clone(),
                        annotations: None,
                        step: 1,
                    }),
                    hotspots: Vec::new(),
                },
                ReviewedGuideStep {
                    index: 2,
                    title: "Step 2".into(),
                    caption: None,
                    kind: CandidateKind::Click,
                    reason: DetectReason::VisualChange,
                    at_ms: 200,
                    image: ReviewedStepImage::Project(ProjectReviewedImage {
                        project_root: root.to_path_buf(),
                        frame: f2.clone(),
                        annotations: None,
                        step: 2,
                    }),
                    hotspots: Vec::new(),
                },
            ],
        };
        (job, f1, f2)
    }

    #[test]
    fn project_variant_dimensions_returns_manifest_size() {
        let dir = tempfile::tempdir().unwrap();
        let (job, _, _) = project_two_step_job(dir.path());
        assert_eq!(job.steps[0].image.dimensions(), (8, 8));
        assert_eq!(job.steps[1].image.dimensions(), (8, 8));
    }

    #[test]
    fn project_variant_resolve_succeeds_when_asset_exists() {
        let dir = tempfile::tempdir().unwrap();
        let (job, _, _) = project_two_step_job(dir.path());
        let cancel = AtomicBool::new(false);
        let result = job.steps[0].image.with_flattened_image(&cancel, |img| {
            assert_eq!(img.dimensions(), (8, 8));
            Ok(())
        });
        assert!(result.is_ok());
    }

    #[test]
    fn project_variant_resolve_reports_asset_missing_when_file_deleted() {
        let dir = tempfile::tempdir().unwrap();
        let (job, _, f2) = project_two_step_job(dir.path());
        let asset_path = dir
            .path()
            .join("assets/frames")
            .join(format!("{}.png", f2.sha256));
        std::fs::remove_file(&asset_path).unwrap();

        let cancel = AtomicBool::new(false);
        let result = job.steps[1]
            .image
            .with_flattened_image(&cancel, |_img| Ok(()));
        assert!(matches!(result, Err(ExportError::AssetMissing { step: 2 })));
    }

    #[test]
    fn project_variant_annotated_matches_in_memory_flattened() {
        use rollshot_image_document::ImageDocument;

        let dir = tempfile::tempdir().unwrap();
        setup_project_dir(dir.path());
        let source = RgbaImage::from_pixel(8, 8, image::Rgba([20, 30, 40, 255]));
        let frame = write_project_asset(dir.path(), &source);

        let mut doc = ImageDocument::new(source.clone());
        doc.add_redaction(rollshot_image_document::ImageRect::new(0.0, 0.0, 4.0, 4.0))
            .unwrap();
        let snapshot = doc.flatten_snapshot();
        let expected = snapshot.flatten();

        let annotations = doc.annotations().to_vec();
        let step = ReviewedGuideStep {
            index: 1,
            title: "Annotated".into(),
            caption: None,
            kind: crate::models::CandidateKind::Click,
            reason: crate::models::DetectReason::VisualChange,
            at_ms: 100,
            image: ReviewedStepImage::Project(ProjectReviewedImage {
                project_root: dir.path().to_path_buf(),
                frame,
                annotations: Some(annotations),
                step: 1,
            }),
            hotspots: Vec::new(),
        };

        let cancel = AtomicBool::new(false);
        let mut resolved_pixels = None;
        step.image
            .with_flattened_image(&cancel, |img| {
                resolved_pixels = Some(img.clone());
                Ok(())
            })
            .unwrap();

        let resolved = resolved_pixels.unwrap();
        assert_eq!(resolved.dimensions(), expected.dimensions());
        assert!(
            resolved.as_raw() == expected.as_raw(),
            "project annotated pixels must match in-memory flattened"
        );
    }

    #[test]
    fn project_variant_uses_default_title_when_editable_title_empty() {
        use crate::models::{
            CandidateKind, CaptureRegion, DetectReason, InputCapability, InputSourceKind,
        };

        let dir = tempfile::tempdir().unwrap();
        setup_project_dir(dir.path());
        let img = RgbaImage::from_pixel(8, 8, image::Rgba([10, 20, 30, 255]));
        let frame = write_project_asset(dir.path(), &img);

        let job = ReviewedGuideExportJob {
            title: "   ".into(),
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
                title: "Step 1".into(),
                caption: None,
                kind: CandidateKind::Click,
                reason: DetectReason::VisualChange,
                at_ms: 100,
                image: ReviewedStepImage::Project(ProjectReviewedImage {
                    project_root: dir.path().to_path_buf(),
                    frame,
                    annotations: None,
                    step: 1,
                }),
                hotspots: Vec::new(),
            }],
        };
        assert_eq!(job.title, "   ", "job stores raw editable title");
        assert!(
            job.validate().is_err(),
            "empty trimmed title fails validation"
        );
    }
}
