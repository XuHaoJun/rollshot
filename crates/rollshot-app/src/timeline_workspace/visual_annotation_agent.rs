#![allow(dead_code)] // Consumed by Tasks 7/8; suppress warnings until wired.

//! Bounded visual annotation suggestion task that runs in the iced async task.
//!
//! After the user consents in the consent dialog, the workspace sends a
//! [`VisualAnnotationTaskInput`] with the selected keyframe. The async task
//! PNG-encodes the image, builds a provider-neutral
//! [`rollshot_agent::domain::AuthorizedModelInput`], dispatches the bounded
//! visual annotation runner, and maps the resulting normalized
//! [`rollshot_agent::VisualAnnotationRunTerminal`] into a
//! [`rollshot_action::VisualAnnotationProposal`] with pixel-space coordinates.
//!
//! `VisualAnnotationTaskResult::Proposal` carries a proposal ready for
//! review; `NoSuggestion` carries a sanitized, user-visible reason when the
//! model declines or the run fails. Coordinate units, provider payloads,
//! and attachment bytes never leave this module.

use rollshot_action::{
    GuideStep, VisualAnnotationPayload, VisualAnnotationProposal, VisualAnnotationProposalId,
    VisualAnnotationSuggestionDraft, VisualAnnotationSuggestionId,
};
use rollshot_agent::domain::{AttachmentDescriptor, AuthorizedModelInput, MediaType};
use rollshot_agent::driver::{AgentConfig, AgentRunner};
use rollshot_agent::runtime::RunCancellation;
use rollshot_agent::{ProviderAdapter, VisualAnnotationDraft, VisualAnnotationRunTerminal};
use rollshot_image_document::{ImagePoint, ImageRect};

/// Inputs the workspace hands to the async visual annotation task. The image
/// is the original retained keyframe (cloned, not borrowed) so the `'static`
/// task can outlive the workspace borrow.
pub(crate) struct VisualAnnotationTaskInput {
    pub run_id: u64,
    pub step: GuideStep,
    pub document_state_id: u64,
    pub image: image::RgbaImage,
}

/// Outcome of one visual annotation run. Returned through `Result` so the
/// workspace can distinguish recoverable suggestion failures from terminal
/// crashes.
#[derive(Debug, Clone)]
pub(crate) enum VisualAnnotationTaskResult {
    Proposal(VisualAnnotationProposal),
    NoSuggestion { reason: Option<String> },
}

/// Consent metadata captured from the consent dialog. Contains no
/// `RgbaImage`, `Vec<u8>`, or `ModelAttachment` — only the identifiers
/// and provider/model names needed for provenance.
#[derive(Debug)]
pub(crate) struct VisualSuggestionConsent {
    pub source: rollshot_action::CandidateId,
    pub keyframe: rollshot_action::FrameId,
    pub provider: String,
    pub model: String,
}

/// Encode the source image as PNG and produce a matching
/// [`AttachmentDescriptor`]. The descriptor's `byte_count` matches the
/// encoded payload exactly so [`AuthorizedModelInput::new`] accepts it.
pub(crate) fn encode_visual_annotation_attachment(
    image: &image::RgbaImage,
) -> Result<(AttachmentDescriptor, Vec<u8>), String> {
    let image_width = image.width();
    let image_height = image.height();
    let mut png = Vec::new();
    image::DynamicImage::ImageRgba8(image.clone())
        .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
        .map_err(|error| format!("PNG encode failed: {error}"))?;
    let byte_count = u64::try_from(png.len())
        .map_err(|_| "PNG payload exceeds authorization limit".to_string())?;
    let descriptor = AttachmentDescriptor {
        media_type: MediaType::Png,
        width: image_width,
        height: image_height,
        byte_count,
    };
    Ok((descriptor, png))
}

/// Scale normalized agent drafts (0.0..=1.0) to pixel-space coordinates
/// and build a [`VisualAnnotationProposal`].
pub(crate) fn suggestion_batch_to_proposal(
    run_id: u64,
    step: &GuideStep,
    document_state_id: u64,
    image_width: u32,
    image_height: u32,
    agent_drafts: Vec<VisualAnnotationDraft>,
) -> Result<VisualAnnotationProposal, rollshot_action::VisualAnnotationProposalError> {
    let drafts: Vec<VisualAnnotationSuggestionDraft> = agent_drafts
        .into_iter()
        .map(|d| suggestion_to_draft(d, image_width, image_height))
        .collect();
    VisualAnnotationProposal::from_agent_drafts(
        VisualAnnotationProposalId(run_id),
        run_id,
        step,
        document_state_id,
        image_width,
        image_height,
        drafts,
    )
}

fn suggestion_to_draft(
    draft: VisualAnnotationDraft,
    image_width: u32,
    image_height: u32,
) -> VisualAnnotationSuggestionDraft {
    match draft {
        VisualAnnotationDraft::NumberCallout {
            id,
            tip,
            bubble,
            confidence,
            rationale,
        } => VisualAnnotationSuggestionDraft {
            id: VisualAnnotationSuggestionId(id as u64),
            payload: VisualAnnotationPayload::NumberCallout {
                tip: ImagePoint::new(tip.x * image_width as f32, tip.y * image_height as f32),
                bubble: ImagePoint::new(
                    bubble.x * image_width as f32,
                    bubble.y * image_height as f32,
                ),
            },
            confidence,
            rationale,
        },
        VisualAnnotationDraft::TextNote {
            id,
            position,
            text,
            confidence,
            rationale,
        } => VisualAnnotationSuggestionDraft {
            id: VisualAnnotationSuggestionId(id as u64),
            payload: VisualAnnotationPayload::TextNote {
                position: ImagePoint::new(
                    position.x * image_width as f32,
                    position.y * image_height as f32,
                ),
                text,
            },
            confidence,
            rationale,
        },
        VisualAnnotationDraft::OpaqueRedaction {
            id,
            bounds,
            confidence,
            rationale,
        } => VisualAnnotationSuggestionDraft {
            id: VisualAnnotationSuggestionId(id as u64),
            payload: VisualAnnotationPayload::OpaqueRedaction {
                bounds: ImageRect {
                    x: bounds.x * image_width as f32,
                    y: bounds.y * image_height as f32,
                    width: bounds.width * image_width as f32,
                    height: bounds.height * image_height as f32,
                },
            },
            confidence,
            rationale,
        },
    }
}

/// Run the bounded visual annotation profile. Caller supplies the provider
/// adapter, the agent model name, and a fresh [`RunCancellation`] owned by
/// the workspace. Returns a typed terminal that the workspace can map into
/// the user-visible state machine.
pub(crate) async fn suggest_visual_annotation_task(
    input: VisualAnnotationTaskInput,
    provider_name: String,
    model: String,
    adapter: Box<dyn ProviderAdapter>,
    cancellation: RunCancellation,
) -> Result<VisualAnnotationTaskResult, String> {
    let VisualAnnotationTaskInput {
        run_id,
        step,
        document_state_id,
        image,
    } = input;
    let image_width = image.width();
    let image_height = image.height();
    let prompt = build_visual_annotation_prompt(&step);
    let (descriptor, png) = match encode_visual_annotation_attachment(&image) {
        Ok(pair) => pair,
        Err(error) => {
            tracing::error!(
                target: "rollshot::action::visual_annotation_agent",
                error = %error,
                "visual annotation PNG encoding failed"
            );
            return Ok(VisualAnnotationTaskResult::NoSuggestion {
                reason: Some(error),
            });
        }
    };
    let authorized = match AuthorizedModelInput::new(
        provider_name.clone(),
        model.clone(),
        prompt,
        vec![descriptor],
        vec![png],
    ) {
        Ok(input) => input,
        Err(error) => {
            tracing::error!(
                target: "rollshot::action::visual_annotation_agent",
                error = %error,
                "visual annotation input authorization failed"
            );
            return Ok(VisualAnnotationTaskResult::NoSuggestion {
                reason: Some(format!("input rejected: {error}")),
            });
        }
    };
    let runner = AgentRunner::new(AgentConfig {
        max_turns: 2,
        ..AgentConfig::default()
    });
    let terminal = runner
        .run_visual_annotation_with_provider(
            authorized,
            &*adapter,
            rollshot_agent::visual_annotation_run_budget(),
            &cancellation,
        )
        .await;
    Ok(map_terminal_to_result(
        terminal,
        run_id,
        &step,
        document_state_id,
        image_width,
        image_height,
    ))
}

fn build_visual_annotation_prompt(step: &GuideStep) -> String {
    format!(
        "Inspect this reviewed Action Guide step and suggest visual annotation overlays \
         (Number Callout, Text Note, or Opaque Redaction) on the attached keyframe. \
         Prefer calling the submit_visual_annotation_suggestions tool. If tool calling \
         is unavailable, return only JSON in the same schema. The image is the only \
         source of truth. Use the step metadata as context only. \
         Step source={}, keyframe={}, title=\"{}\"",
        step.source, step.keyframe, step.title,
    )
}

fn map_terminal_to_result(
    terminal: VisualAnnotationRunTerminal,
    run_id: u64,
    step: &GuideStep,
    document_state_id: u64,
    image_width: u32,
    image_height: u32,
) -> VisualAnnotationTaskResult {
    match terminal {
        VisualAnnotationRunTerminal::Suggested(drafts) => {
            match suggestion_batch_to_proposal(
                run_id,
                step,
                document_state_id,
                image_width,
                image_height,
                drafts,
            ) {
                Ok(proposal) => VisualAnnotationTaskResult::Proposal(proposal),
                Err(error) => {
                    tracing::warn!(
                        target: "rollshot::action::visual_annotation_agent",
                        error = %error,
                        "visual annotation draft failed proposal validation"
                    );
                    VisualAnnotationTaskResult::NoSuggestion {
                        reason: Some(format!("draft rejected: {error}")),
                    }
                }
            }
        }
        VisualAnnotationRunTerminal::Cancelled => {
            tracing::info!(
                target: "rollshot::action::visual_annotation_agent",
                run_id,
                "visual annotation run cancelled"
            );
            VisualAnnotationTaskResult::NoSuggestion { reason: None }
        }
        VisualAnnotationRunTerminal::BudgetExhausted { dimension } => {
            tracing::warn!(
                target: "rollshot::action::visual_annotation_agent",
                run_id,
                dimension = ?dimension,
                "visual annotation run budget exhausted"
            );
            VisualAnnotationTaskResult::NoSuggestion {
                reason: Some("Visual annotation suggestion budget exhausted.".to_string()),
            }
        }
        VisualAnnotationRunTerminal::ProviderFailure => {
            tracing::warn!(
                target: "rollshot::action::visual_annotation_agent",
                run_id,
                "visual annotation provider stream failed"
            );
            VisualAnnotationTaskResult::NoSuggestion {
                reason: Some("Visual annotation provider failed.".to_string()),
            }
        }
        VisualAnnotationRunTerminal::ProtocolFailure => {
            tracing::warn!(
                target: "rollshot::action::visual_annotation_agent",
                run_id,
                "visual annotation model did not return a usable suggestion"
            );
            VisualAnnotationTaskResult::NoSuggestion {
                reason: Some(
                    "Visual annotation model did not return a usable suggestion.".to_string(),
                ),
            }
        }
        VisualAnnotationRunTerminal::NoSuggestion(_) => {
            tracing::info!(
                target: "rollshot::action::visual_annotation_agent",
                run_id,
                "visual annotation model declined to suggest"
            );
            VisualAnnotationTaskResult::NoSuggestion { reason: None }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{Rgba, RgbaImage};
    use rollshot_action::{CandidateKind, DetectReason};
    use rollshot_agent::NormalizedPoint;
    use rollshot_agent::NormalizedRect;

    fn step() -> GuideStep {
        GuideStep {
            index: 1,
            title: "Open Settings".to_string(),
            caption: "The settings panel appears.".to_string(),
            kind: CandidateKind::Click,
            reason: DetectReason::ClickConfirmed,
            at_ms: 120,
            keyframe: 7,
            nearby: vec![6, 7, 8],
            source: 10,
        }
    }

    fn agent_batch() -> Vec<VisualAnnotationDraft> {
        vec![
            VisualAnnotationDraft::NumberCallout {
                id: 1,
                tip: NormalizedPoint { x: 0.5, y: 0.5 },
                bubble: NormalizedPoint { x: 0.2, y: 0.3 },
                confidence: 0.9,
                rationale: Some("button center".into()),
            },
            VisualAnnotationDraft::TextNote {
                id: 2,
                position: NormalizedPoint { x: 0.75, y: 0.1 },
                text: "Save button".into(),
                confidence: 0.8,
                rationale: None,
            },
            VisualAnnotationDraft::OpaqueRedaction {
                id: 3,
                bounds: NormalizedRect {
                    x: 0.1,
                    y: 0.2,
                    width: 0.3,
                    height: 0.4,
                },
                confidence: 0.7,
                rationale: None,
            },
        ]
    }

    #[test]
    fn normalized_agent_batch_becomes_valid_core_proposal() {
        let proposal = suggestion_batch_to_proposal(7, &step(), 12, 400, 200, agent_batch())
            .expect("proposal");
        assert_eq!(proposal.suggestions.len(), 3);
        assert_eq!(proposal.suggestions[0].base.image_width, 400);
        assert_eq!(proposal.suggestions[0].base.image_height, 200);
        assert_eq!(proposal.suggestions[0].base.document_state_id, 12);
        assert_eq!(proposal.suggestions[0].base.step_source, 10);
        assert_eq!(proposal.suggestions[0].base.keyframe, 7);
        assert_eq!(proposal.run_id, 7);
    }

    #[test]
    fn callout_coordinates_are_scaled_to_pixel_space() {
        let proposal =
            suggestion_batch_to_proposal(1, &step(), 1, 400, 200, agent_batch()).expect("proposal");
        let callout = match &proposal.suggestions[0].payload {
            VisualAnnotationPayload::NumberCallout { tip, bubble } => (tip, bubble),
            other => panic!("expected NumberCallout, got {other:?}"),
        };
        assert!((callout.0.x - 200.0).abs() < 1e-4);
        assert!((callout.0.y - 100.0).abs() < 1e-4);
        assert!((callout.1.x - 80.0).abs() < 1e-4);
        assert!((callout.1.y - 60.0).abs() < 1e-4);
    }

    #[test]
    fn note_coordinates_are_scaled_to_pixel_space() {
        let proposal =
            suggestion_batch_to_proposal(1, &step(), 1, 400, 200, agent_batch()).expect("proposal");
        let note = match &proposal.suggestions[1].payload {
            VisualAnnotationPayload::TextNote { position, text } => (position, text),
            other => panic!("expected TextNote, got {other:?}"),
        };
        assert!((note.0.x - 300.0).abs() < 1e-4);
        assert!((note.0.y - 20.0).abs() < 1e-4);
        assert_eq!(note.1, "Save button");
    }

    #[test]
    fn redaction_coordinates_are_scaled_to_pixel_space() {
        let proposal =
            suggestion_batch_to_proposal(1, &step(), 1, 400, 200, agent_batch()).expect("proposal");
        let rect = match &proposal.suggestions[2].payload {
            VisualAnnotationPayload::OpaqueRedaction { bounds } => bounds,
            other => panic!("expected OpaqueRedaction, got {other:?}"),
        };
        assert!((rect.x - 40.0).abs() < 1e-4);
        assert!((rect.y - 40.0).abs() < 1e-4);
        assert!((rect.width - 120.0).abs() < 1e-4);
        assert!((rect.height - 80.0).abs() < 1e-4);
    }

    #[test]
    fn single_suggestion_batch_converts_to_proposal() {
        let single = vec![VisualAnnotationDraft::TextNote {
            id: 1,
            position: NormalizedPoint { x: 0.5, y: 0.5 },
            text: "note".into(),
            confidence: 0.8,
            rationale: None,
        }];
        let proposal =
            suggestion_batch_to_proposal(3, &step(), 5, 800, 600, single).expect("proposal");
        assert_eq!(proposal.suggestions.len(), 1);
        assert_eq!(proposal.suggestions[0].base.image_width, 800);
        assert_eq!(proposal.suggestions[0].base.image_height, 600);
    }

    #[test]
    fn encode_visual_annotation_attachment_produces_valid_png() {
        let image = RgbaImage::from_fn(4, 3, |x, y| {
            if (x + y) % 2 == 0 {
                Rgba([10, 20, 30, 255])
            } else {
                Rgba([40, 50, 60, 255])
            }
        });

        let (descriptor, png) =
            encode_visual_annotation_attachment(&image).expect("encoding succeeds");

        assert_eq!(descriptor.width, 4);
        assert_eq!(descriptor.height, 3);
        assert!(matches!(descriptor.media_type, MediaType::Png));
        assert_eq!(
            descriptor.byte_count,
            u64::try_from(png.len()).expect("png fits in u64")
        );
        assert_eq!(&png[..8], &[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A]);
    }

    #[test]
    fn consent_struct_contains_no_image_or_attachment_types() {
        let _consent = VisualSuggestionConsent {
            source: 10,
            keyframe: 7,
            provider: "test".into(),
            model: "test".into(),
        };
    }
}
