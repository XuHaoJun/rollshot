use std::sync::Arc;

use rollshot_action::{
    ExportError, GuideHotspot, NormalizedRect, ReviewedGuideExportJob, ReviewedGuideStep,
    ReviewedStepImage,
};
use rollshot_image_document::Annotation;

use super::TimelineWorkspace;

pub(crate) fn build_reviewed_export_job(
    state: &TimelineWorkspace,
) -> Result<ReviewedGuideExportJob, ExportError> {
    let mut steps = Vec::new();
    for (i, step) in state.guide.steps().iter().enumerate() {
        let frame = state
            .store
            .retained(step.keyframe)
            .ok_or(ExportError::MissingKeyframe { index: i + 1 })?;
        let (w, h) = frame.image.dimensions();

        let image = match state.presentation.doc(step.source) {
            Some(doc)
                if doc.keyframe == step.keyframe && !doc.document.annotations().is_empty() =>
            {
                ReviewedStepImage::Annotated(doc.document.flatten_snapshot())
            }
            _ => ReviewedStepImage::Retained(Arc::clone(&frame.image)),
        };

        let hotspots = match state.presentation.doc(step.source) {
            Some(doc) if doc.keyframe == step.keyframe => build_hotspots(doc, w, h),
            _ => Vec::new(),
        };

        let caption = {
            let trimmed = step.caption.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed.to_string())
            }
        };

        steps.push(ReviewedGuideStep {
            index: i + 1,
            title: step.title.clone(),
            caption,
            kind: step.kind,
            reason: step.reason,
            at_ms: step.at_ms,
            image,
            hotspots,
        });
    }

    let job = ReviewedGuideExportJob {
        title: state.guide.effective_title().to_string(),
        region: state.region,
        input_source: state.source_kind,
        input_capability: state.capability,
        steps,
    };
    job.validate()?;
    Ok(job)
}

fn build_hotspots(
    doc: &super::annotation::StepAnnotationDocument,
    width: u32,
    height: u32,
) -> Vec<GuideHotspot> {
    let mut hotspots = Vec::new();
    for item in doc.document.navigator_items() {
        let Some(annotation) = doc.document.annotation(item.id) else {
            continue;
        };
        let explanation = match annotation {
            Annotation::TextNote { text, .. } => text.trim(),
            Annotation::NumberCallout { id, .. } => doc
                .explanations
                .get(id)
                .map(String::as_str)
                .unwrap_or("")
                .trim(),
            _ => "",
        };
        if explanation.is_empty() {
            continue;
        }
        let bounds = normalize_and_clamp(annotation, width, height);
        hotspots.push(GuideHotspot {
            annotation_id: item.id.0,
            bounds,
            explanation: explanation.to_string(),
        });
    }
    hotspots
}

fn normalize_and_clamp(annotation: &Annotation, width: u32, height: u32) -> NormalizedRect {
    let r = rollshot_image_document::annotation_bounds(annotation);
    let w = width as f32;
    let h = height as f32;
    let x = (r.x / w).clamp(0.0, 1.0);
    let y = (r.y / h).clamp(0.0, 1.0);
    let right = ((r.x + r.width) / w).clamp(0.0, 1.0);
    let bottom = ((r.y + r.height) / h).clamp(0.0, 1.0);
    NormalizedRect {
        x,
        y,
        width: right - x,
        height: bottom - y,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rollshot_action::{CaptureRegion, InputCapability, InputSourceKind, ReviewedStepImage};
    use rollshot_image_document::ImagePoint;

    fn real_workspace() -> TimelineWorkspace {
        TimelineWorkspace::new(
            super::super::tests::recording_from_frames(),
            CaptureRegion {
                x: 0,
                y: 0,
                width: 32,
                height: 32,
            },
            InputCapability::SemanticEvents,
            InputSourceKind::LinuxEvdev,
        )
    }

    #[test]
    fn job_contains_text_notes_and_only_explained_callouts_in_navigator_order() {
        let mut state = real_workspace();
        let step = state.guide.steps()[0].clone();
        let doc = state
            .presentation
            .document_for_step(&step, &state.store)
            .unwrap();
        let late = doc
            .document
            .add_number_callout(ImagePoint::new(20.0, 20.0), ImagePoint::new(24.0, 24.0));
        doc.document
            .add_text_note(ImagePoint::new(2.0, 2.0), "First note".into())
            .unwrap();
        let silent = doc
            .document
            .add_number_callout(ImagePoint::new(10.0, 10.0), ImagePoint::new(14.0, 14.0));
        state
            .presentation
            .set_explanation(step.source, late, "Second explanation".into());
        state
            .presentation
            .set_explanation(step.source, silent, "   ".into());

        let job = build_reviewed_export_job(&state).unwrap();

        assert_eq!(job.steps[0].hotspots.len(), 2);
        assert_eq!(job.steps[0].hotspots[0].explanation, "First note");
        assert_eq!(job.steps[0].hotspots[1].explanation, "Second explanation");
        assert!(matches!(
            job.steps[0].image,
            ReviewedStepImage::Annotated(_)
        ));
    }

    #[test]
    fn job_without_matching_annotations_shares_retained_keyframe() {
        let state = real_workspace();
        let frame = Arc::clone(
            &state
                .store
                .retained(state.guide.steps()[0].keyframe)
                .unwrap()
                .image,
        );
        let job = build_reviewed_export_job(&state).unwrap();
        let ReviewedStepImage::Retained(exported) = &job.steps[0].image else {
            panic!("retained")
        };
        assert!(Arc::ptr_eq(exported, &frame));
    }

    #[test]
    fn job_is_isolated_from_edits_after_export_click() {
        let mut state = real_workspace();
        let job = build_reviewed_export_job(&state).unwrap();
        let exported_title = job.title.clone();
        let exported_step_title = job.steps[0].title.clone();

        state.guide.set_title("Edited after click".into());
        assert!(state.guide.rename(1, "Changed later".into()));

        assert_eq!(job.title, exported_title);
        assert_eq!(job.steps[0].title, exported_step_title);
        job.validate().unwrap();
    }
}
