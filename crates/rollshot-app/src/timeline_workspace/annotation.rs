#![expect(dead_code)]

use std::collections::{BTreeMap, BTreeSet};

use iced::widget::image;
use rollshot_action::{CandidateId, FrameId, FrameStore, GuideStep};
use rollshot_image_document::{ImageDocument, ImagePoint};

pub(crate) struct StepAnnotationDocument {
    pub source: CandidateId,
    pub keyframe: FrameId,
    pub document: ImageDocument,
}

#[derive(Default)]
pub(crate) struct ActionGuidePresentation {
    docs: BTreeMap<CandidateId, StepAnnotationDocument>,
}

impl ActionGuidePresentation {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn document_for_step(
        &mut self,
        step: &GuideStep,
        store: &FrameStore,
    ) -> Option<&mut StepAnnotationDocument> {
        let needs_new = self
            .docs
            .get(&step.source)
            .is_none_or(|doc| doc.keyframe != step.keyframe);
        if needs_new {
            let frame = store.retained(step.keyframe)?;
            self.docs.insert(
                step.source,
                StepAnnotationDocument {
                    source: step.source,
                    keyframe: step.keyframe,
                    document: ImageDocument::new(frame.image.clone()),
                },
            );
        }
        self.docs.get_mut(&step.source)
    }

    pub(crate) fn doc(&self, source: CandidateId) -> Option<&StepAnnotationDocument> {
        self.docs.get(&source)
    }

    pub(crate) fn has_annotations(&self, source: CandidateId) -> bool {
        self.docs
            .get(&source)
            .is_some_and(|doc| !doc.document.annotations().is_empty())
    }

    pub(crate) fn clear_for_source(&mut self, source: CandidateId) -> bool {
        self.docs.remove(&source).is_some()
    }

    pub(crate) fn retain_sources(&mut self, sources: impl IntoIterator<Item = CandidateId>) {
        let keep: BTreeSet<_> = sources.into_iter().collect();
        self.docs.retain(|source, _| keep.contains(source));
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum AnnotationDraft {
    Number { tip: ImagePoint, bubble: ImagePoint },
}

pub(crate) struct StepAnnotationSession {
    pub source: CandidateId,
    pub keyframe: FrameId,
    pub handle: image::Handle,
    pub width: u32,
    pub height: u32,
    pub draft: Option<AnnotationDraft>,
}

impl StepAnnotationSession {
    pub(crate) fn new(source: CandidateId, keyframe: FrameId, image: &::image::RgbaImage) -> Self {
        Self {
            source,
            keyframe,
            handle: super::build_handle(image),
            width: image.width(),
            height: image.height(),
            draft: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rollshot_action::{
        CandidateKind, CandidateStep, DetectReason, FrameStore, Guide, StoreConfig,
    };

    fn frame_store_with_two_frames() -> FrameStore {
        let mut store = FrameStore::new(StoreConfig::default());
        let first = store.ingest(
            ::image::RgbaImage::from_pixel(8, 8, ::image::Rgba([0, 0, 0, 255])),
            0,
        );
        let second = store.ingest(
            ::image::RgbaImage::from_pixel(8, 8, ::image::Rgba([255, 255, 255, 255])),
            100,
        );
        store.retain_window(first);
        store.retain_window(second);
        store
    }

    fn guide() -> Guide {
        Guide::from_candidates(vec![CandidateStep {
            id: 42,
            kind: CandidateKind::Click,
            reason: DetectReason::VisualChange,
            at_ms: 100,
            keyframe: 0,
            nearby: vec![0, 1],
        }])
    }

    #[test]
    fn document_for_step_is_keyed_by_source_and_uses_current_keyframe() {
        let store = frame_store_with_two_frames();
        let guide = guide();
        let step = &guide.steps()[0];
        let mut presentation = ActionGuidePresentation::new();

        let doc = presentation
            .document_for_step(step, &store)
            .expect("document exists");

        assert_eq!(doc.source, 42);
        assert_eq!(doc.keyframe, 0);
        assert_eq!(doc.document.source().dimensions(), (8, 8));
        assert!(!presentation.has_annotations(step.source));
    }

    #[test]
    fn clear_for_keyframe_change_removes_only_matching_step() {
        let store = frame_store_with_two_frames();
        let guide = guide();
        let step = &guide.steps()[0];
        let mut presentation = ActionGuidePresentation::new();
        let doc = presentation.document_for_step(step, &store).unwrap();
        doc.document.add_number_callout(
            rollshot_image_document::ImagePoint::new(1.0, 1.0),
            rollshot_image_document::ImagePoint::new(4.0, 4.0),
        );

        assert!(presentation.clear_for_source(step.source));
        assert!(!presentation.has_annotations(step.source));
        assert!(!presentation.clear_for_source(step.source));
    }

    #[test]
    fn retain_sources_prunes_deleted_steps() {
        let store = frame_store_with_two_frames();
        let guide = guide();
        let step = &guide.steps()[0];
        let mut presentation = ActionGuidePresentation::new();
        presentation.document_for_step(step, &store).unwrap();

        presentation.retain_sources(std::iter::empty());

        assert!(!presentation.has_annotations(step.source));
    }
}
