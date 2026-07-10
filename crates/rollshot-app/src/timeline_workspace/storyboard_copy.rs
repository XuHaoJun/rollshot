use rollshot_action::StoryboardError;

pub(crate) struct StoryboardCopyStep {
    pub index: usize,
    pub title: String,
    pub caption: Option<String>,
    pub image: image::RgbaImage,
}

pub(crate) struct StoryboardCopyInput {
    pub steps: Vec<StoryboardCopyStep>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct StoryboardCopyResult {
    pub width: u32,
    pub height: u32,
    pub step_count: usize,
}

pub(crate) fn snapshot_storyboard(
    guide: &rollshot_action::Guide,
    store: &rollshot_action::FrameStore,
    presentation: &super::annotation::ActionGuidePresentation,
) -> Result<StoryboardCopyInput, StoryboardError> {
    if guide.is_empty() {
        return Err(StoryboardError::Empty);
    }

    let mut steps = Vec::with_capacity(guide.steps().len());
    for (i, step) in guide.steps().iter().enumerate() {
        let frame = store
            .retained(step.keyframe)
            .ok_or(StoryboardError::KeyframeMissing { index: i + 1 })?;
        let image = match presentation.doc(step.source) {
            Some(doc)
                if doc.keyframe == step.keyframe && !doc.document.annotations().is_empty() =>
            {
                doc.document.flatten()
            }
            _ => frame.image.clone(),
        };
        let caption = {
            let caption = step.caption.trim();
            (!caption.is_empty()).then(|| caption.to_string())
        };
        steps.push(StoryboardCopyStep {
            index: step.index,
            title: step.title.clone(),
            caption,
            image,
        });
    }

    Ok(StoryboardCopyInput { steps })
}

pub(crate) fn render_storyboard_input(
    input: &StoryboardCopyInput,
    options: rollshot_action::StoryboardOptions,
) -> Result<rollshot_action::StoryboardRenderResult, StoryboardError> {
    let steps = input
        .steps
        .iter()
        .map(|step| rollshot_action::StoryboardStep {
            index: step.index,
            title: &step.title,
            caption: step.caption.as_deref(),
            image: &step.image,
        })
        .collect::<Vec<_>>();
    rollshot_action::render_storyboard_steps(&steps, options)
}

pub(crate) fn render_and_copy_with(
    input: StoryboardCopyInput,
    copy: impl FnOnce(&image::RgbaImage) -> Result<(), String>,
) -> Result<StoryboardCopyResult, String> {
    let result = render_storyboard_input(&input, rollshot_action::StoryboardOptions::default())
        .map_err(|e| format!("Couldn't render Storyboard: {e}"))?;
    copy(&result.image).map_err(|e| format!("Couldn't copy Storyboard: {e}"))?;
    Ok(StoryboardCopyResult {
        width: result.width,
        height: result.height,
        step_count: result.step_count,
    })
}

pub(crate) async fn render_and_copy(
    input: StoryboardCopyInput,
) -> Result<StoryboardCopyResult, String> {
    tokio::task::spawn_blocking(move || {
        render_and_copy_with(input, crate::image_clipboard::copy_rgba_image)
    })
    .await
    .map_err(|_| "Storyboard copy worker failed.".to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::RgbaImage;

    fn workspace_with_steps(count: usize) -> super::super::TimelineWorkspace {
        super::super::TimelineWorkspace::new(
            crate::timeline_workspace::tests::synthetic_recording(count),
            rollshot_action::CaptureRegion {
                x: 0,
                y: 0,
                width: 32,
                height: 32,
            },
            rollshot_action::InputCapability::SemanticEvents,
            rollshot_action::InputSourceKind::LinuxEvdev,
        )
    }

    fn real_workspace() -> super::super::TimelineWorkspace {
        super::super::TimelineWorkspace::new(
            crate::timeline_workspace::tests::recording_from_frames(),
            rollshot_action::CaptureRegion {
                x: 0,
                y: 0,
                width: 32,
                height: 32,
            },
            rollshot_action::InputCapability::SemanticEvents,
            rollshot_action::InputSourceKind::LinuxEvdev,
        )
    }

    fn add_callout_to_first_step(state: &mut super::super::TimelineWorkspace) {
        let step = state.guide.steps()[0].clone();
        let document = state
            .presentation
            .document_for_step(&step, &state.store)
            .unwrap();
        document.document.add_number_callout(
            rollshot_image_document::ImagePoint::new(2.0, 2.0),
            rollshot_image_document::ImagePoint::new(8.0, 8.0),
        );
    }

    fn one_step_input() -> StoryboardCopyInput {
        let image = RgbaImage::from_raw(
            4,
            4,
            vec![
                255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 0, 255, 255, 0, 0, 255,
                0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 0, 255, 255, 0, 0, 255, 0, 255, 0, 255,
                0, 0, 255, 255, 255, 255, 0, 255, 255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255,
                255, 255, 0, 255,
            ],
        )
        .unwrap();
        StoryboardCopyInput {
            steps: vec![StoryboardCopyStep {
                index: 1,
                title: "Test Step".to_string(),
                caption: Some("A test caption".to_string()),
                image,
            }],
        }
    }

    #[test]
    fn snapshot_preserves_reviewed_order_titles_and_trimmed_captions() {
        let mut state = real_workspace();
        let step_idx = state.guide.steps()[0].index;
        state.guide.rename(step_idx, "Open Settings".into());
        state
            .guide
            .set_caption(step_idx, "  Show the panel.  ".into());

        let input = snapshot_storyboard(&state.guide, &state.store, &state.presentation).unwrap();

        assert_eq!(
            input
                .steps
                .iter()
                .map(|step| step.index)
                .collect::<Vec<_>>(),
            state
                .guide
                .steps()
                .iter()
                .map(|s| s.index)
                .collect::<Vec<_>>()
        );
        assert_eq!(input.steps[0].title, "Open Settings");
        assert_eq!(input.steps[0].caption.as_deref(), Some("Show the panel."));
    }

    #[test]
    fn snapshot_flattens_annotations_without_mutating_document() {
        let mut state = real_workspace();
        add_callout_to_first_step(&mut state);
        let source = state.guide.steps()[0].source;
        let before = state.presentation.doc(source).unwrap().document.state_id();

        let input = snapshot_storyboard(&state.guide, &state.store, &state.presentation).unwrap();

        assert_ne!(
            input.steps[0].image,
            state
                .store
                .retained(state.guide.steps()[0].keyframe)
                .unwrap()
                .image
        );
        assert_eq!(
            state.presentation.doc(source).unwrap().document.state_id(),
            before
        );
    }

    #[test]
    fn snapshot_empty_guide_returns_empty_error() {
        let state = workspace_with_steps(0);

        let result = snapshot_storyboard(&state.guide, &state.store, &state.presentation);

        assert!(matches!(result, Err(StoryboardError::Empty)));
    }

    #[test]
    fn snapshot_missing_keyframe_returns_keyframe_missing_error() {
        let state = workspace_with_steps(1);

        let result = snapshot_storyboard(&state.guide, &state.store, &state.presentation);

        assert!(matches!(
            result,
            Err(StoryboardError::KeyframeMissing { index: 1 })
        ));
    }

    #[test]
    fn copy_pipeline_matches_export_quality_pixels() {
        let input = one_step_input();
        let expected =
            render_storyboard_input(&input, rollshot_action::StoryboardOptions::default()).unwrap();
        let captured = std::rc::Rc::new(std::cell::RefCell::new(None));
        let output = std::rc::Rc::clone(&captured);

        let result = render_and_copy_with(input, move |image| {
            *output.borrow_mut() = Some(image.clone());
            Ok(())
        })
        .unwrap();

        assert_eq!(captured.borrow().as_ref(), Some(&expected.image));
        assert_eq!(
            (result.width, result.height),
            (expected.width, expected.height)
        );
        assert_eq!(result.step_count, expected.step_count);
    }

    #[test]
    fn renderer_failure_does_not_call_clipboard() {
        let called = std::cell::Cell::new(false);
        let result = render_and_copy_with(StoryboardCopyInput { steps: vec![] }, |_| {
            called.set(true);
            Ok(())
        });
        assert!(result.is_err());
        assert!(!called.get());
    }

    #[test]
    fn clipboard_failure_prefixes_error() {
        let input = one_step_input();
        let result = render_and_copy_with(input, |_| Err("clipboard busy".to_string()));
        assert!(result.unwrap_err().starts_with("Couldn't copy Storyboard:"));
    }
}
