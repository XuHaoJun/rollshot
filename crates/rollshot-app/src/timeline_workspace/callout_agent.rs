//! Bounded callout suggestion task that runs in the iced async task.
//!
//! The Timeline Workspace sends a [`CalloutTaskInput`] describing the
//! selected step, the document `state_id`, and a clone of the original
//! retained keyframe. The async task PNG-encodes the image, builds a
//! provider-neutral [`rollshot_agent::domain::AuthorizedModelInput`],
//! dispatches the bounded `run_callout_with_provider` runner, and maps
//! the resulting [`rollshot_agent::callout::CalloutRunTerminal`] into
//! the Rollshot-owned [`CalloutTaskResult`].
//!
//! `CalloutTaskResult::Proposal` carries a [`rollshot_action::CalloutProposal`]
//! ready for review; `NoSuggestion` carries a sanitized, user-visible reason
//! when the model declines or the run fails. Coordinate units, provider
//! payloads, and attachment bytes never leave this module.

use rollshot_action::{CalloutProposal, CalloutProposalId, CalloutSuggestionDraft, GuideStep};
use rollshot_agent::callout::{CalloutAgentSuggestion, CalloutNoSuggestion, CalloutRunTerminal};
use rollshot_agent::domain::{AttachmentDescriptor, AuthorizedModelInput, MediaType};
use rollshot_agent::driver::{AgentConfig, AgentRunner};
use rollshot_agent::runtime::RunCancellation;
use rollshot_agent::ProviderAdapter;
use rollshot_image_document::ImagePoint;
use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub(crate) struct CalloutPromptStep {
    pub index: usize,
    pub source: rollshot_action::CandidateId,
    pub title: String,
    pub caption: String,
    pub kind: String,
}

/// Inputs the workspace hands to the async callout task. The image is the
/// original retained keyframe (cloned, not borrowed) so the `'static` task
/// can outlive the workspace borrow.
pub(crate) struct CalloutTaskInput {
    pub run_id: u64,
    pub step: GuideStep,
    pub document_state_id: u64,
    pub image: image::RgbaImage,
}

/// Outcome of one callout run. Returned through `Result` so the workspace
/// can distinguish recoverable suggestion failures from terminal crashes.
#[derive(Debug, Clone)]
pub(crate) enum CalloutTaskResult {
    Proposal(CalloutProposal),
    NoSuggestion { reason: Option<String> },
}

/// Build the prompt the agent sees. Carries the reviewed step's metadata
/// (source, index, title, caption, kind) as context only — the image is the
/// source of truth for the suggested tip. The prompt is intentionally free
/// of any raw typed text field.
pub(crate) fn build_callout_prompt(step: &GuideStep) -> String {
    let payload = CalloutPromptStep {
        index: step.index,
        source: step.source,
        title: step.title.clone(),
        caption: step.caption.clone(),
        kind: format!("{:?}", step.kind),
    };
    let json = serde_json::to_string(&payload).unwrap_or_else(|_| "{}".to_string());
    format!(
        "Inspect this reviewed Action Guide step and suggest exactly one Number Callout tip \
on the attached keyframe. Prefer calling the submit_callout_suggestion tool. If tool \
calling is unavailable, return only JSON in the same schema. The image is the only \
source of truth; do not invent raw typed text. Use the source/index/title/caption/kind as \
context only. Step: {json}"
    )
}

/// Encode the source image as PNG and produce a matching
/// [`AttachmentDescriptor`]. The descriptor's `byte_count` matches the
/// encoded payload exactly so [`AuthorizedModelInput::new`] accepts it.
pub(crate) fn encode_callout_attachment(
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

/// Run the bounded callout profile. Caller supplies the provider adapter,
/// the agent model name, and a fresh [`RunCancellation`] owned by the
/// workspace. Returns a typed terminal that the workspace can map into
/// the user-visible state machine.
pub(crate) async fn suggest_callout_task(
    input: CalloutTaskInput,
    provider_name: String,
    model: String,
    adapter: Box<dyn ProviderAdapter>,
    cancellation: RunCancellation,
) -> Result<CalloutTaskResult, String> {
    let CalloutTaskInput {
        run_id,
        step,
        document_state_id,
        image,
    } = input;
    let image_width = image.width();
    let image_height = image.height();
    let prompt = build_callout_prompt(&step);
    let (descriptor, png) = match encode_callout_attachment(&image) {
        Ok(pair) => pair,
        Err(error) => {
            tracing::error!(
                target: "rollshot::action::callout_agent",
                error = %error,
                "callout PNG encoding failed"
            );
            return Err(error);
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
                target: "rollshot::action::callout_agent",
                error = %error,
                "callout input authorization failed"
            );
            return Err(format!("input rejected: {error}"));
        }
    };
    let runner = AgentRunner::new(AgentConfig {
        max_turns: 2,
        ..AgentConfig::default()
    });
    let terminal = runner
        .run_callout_with_provider(
            authorized,
            &*adapter,
            rollshot_agent::callout::callout_run_budget(),
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

fn map_terminal_to_result(
    terminal: CalloutRunTerminal,
    run_id: u64,
    step: &GuideStep,
    document_state_id: u64,
    image_width: u32,
    image_height: u32,
) -> CalloutTaskResult {
    match terminal {
        CalloutRunTerminal::Suggested(suggestion) => suggestion_to_proposal(
            &suggestion,
            run_id,
            step,
            document_state_id,
            image_width,
            image_height,
        ),
        CalloutRunTerminal::NoSuggestion(CalloutNoSuggestion::NoClearTarget { reason }) => {
            CalloutTaskResult::NoSuggestion { reason }
        }
        CalloutRunTerminal::Cancelled => {
            tracing::info!(
                target: "rollshot::action::callout_agent",
                run_id,
                "callout run cancelled"
            );
            CalloutTaskResult::NoSuggestion { reason: None }
        }
        CalloutRunTerminal::BudgetExhausted { dimension } => {
            tracing::warn!(
                target: "rollshot::action::callout_agent",
                run_id,
                dimension = ?dimension,
                "callout run budget exhausted"
            );
            CalloutTaskResult::NoSuggestion {
                reason: Some("Callout suggestion budget exhausted.".to_string()),
            }
        }
        CalloutRunTerminal::ProviderFailure => {
            tracing::warn!(
                target: "rollshot::action::callout_agent",
                run_id,
                "callout provider stream failed"
            );
            CalloutTaskResult::NoSuggestion {
                reason: Some("Callout provider failed.".to_string()),
            }
        }
        CalloutRunTerminal::ProtocolFailure => {
            tracing::warn!(
                target: "rollshot::action::callout_agent",
                run_id,
                "callout model did not return a usable suggestion"
            );
            CalloutTaskResult::NoSuggestion {
                reason: Some("Callout model did not return a usable suggestion.".to_string()),
            }
        }
    }
}

fn suggestion_to_proposal(
    suggestion: &CalloutAgentSuggestion,
    run_id: u64,
    step: &GuideStep,
    document_state_id: u64,
    image_width: u32,
    image_height: u32,
) -> CalloutTaskResult {
    let tip = ImagePoint::new(
        suggestion.x * image_width as f32,
        suggestion.y * image_height as f32,
    );
    let draft = CalloutSuggestionDraft {
        tip,
        confidence: suggestion.confidence,
        rationale: suggestion.rationale.clone(),
    };
    match CalloutProposal::from_agent_draft(
        CalloutProposalId(run_id),
        run_id,
        step,
        document_state_id,
        image_width,
        image_height,
        draft,
    ) {
        Ok(proposal) => CalloutTaskResult::Proposal(proposal),
        Err(error) => {
            tracing::warn!(
                target: "rollshot::action::callout_agent",
                error = %error,
                "callout draft failed proposal validation"
            );
            CalloutTaskResult::NoSuggestion {
                reason: Some(format!("draft rejected: {error}")),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{Rgba, RgbaImage};
    use rollshot_action::{CandidateKind, DetectReason};

    fn step() -> GuideStep {
        GuideStep {
            index: 1,
            title: "Open Settings".to_string(),
            caption: "The settings panel appears.".to_string(),
            kind: CandidateKind::Click,
            reason: DetectReason::ClickConfirmed,
            at_ms: 120,
            keyframe: 1,
            nearby: vec![0, 1, 2],
            source: 10,
        }
    }

    fn typed_step() -> GuideStep {
        GuideStep {
            index: 2,
            title: "Enter text".to_string(),
            caption: "The user types a query.".to_string(),
            kind: CandidateKind::Typing,
            reason: DetectReason::TypingSettled,
            at_ms: 240,
            keyframe: 3,
            nearby: vec![3],
            source: 11,
        }
    }

    fn rgba_image_2x3() -> RgbaImage {
        RgbaImage::from_fn(2, 3, |x, y| {
            if (x + y) % 2 == 0 {
                Rgba([10, 20, 30, 255])
            } else {
                Rgba([40, 50, 60, 255])
            }
        })
    }

    #[test]
    fn build_callout_prompt_includes_step_metadata_without_typed_text() {
        let prompt = build_callout_prompt(&step());

        assert!(prompt.contains("\"index\":1"), "prompt = {prompt}");
        assert!(prompt.contains("\"source\":10"), "prompt = {prompt}");
        assert!(
            prompt.contains("\"title\":\"Open Settings\""),
            "prompt = {prompt}"
        );
        assert!(
            prompt.contains("\"caption\":\"The settings panel appears.\""),
            "prompt = {prompt}"
        );
        assert!(prompt.contains("\"kind\":\"Click\""), "prompt = {prompt}");
        // The prompt must not include a raw typed text field. Check for the
        // JSON field name and the snake/camel variants it would naturally
        // appear in.
        assert!(!prompt.contains("\"typed_text\""), "prompt = {prompt}");
        assert!(!prompt.contains("\"typedText\""), "prompt = {prompt}");
        assert!(!prompt.contains("\"text\":"), "prompt = {prompt}");
    }

    #[test]
    fn build_callout_prompt_for_typing_step_omits_typed_text_field() {
        let prompt = build_callout_prompt(&typed_step());

        assert!(prompt.contains("\"kind\":\"Typing\""), "prompt = {prompt}");
        assert!(prompt.contains("\"source\":11"), "prompt = {prompt}");
        assert!(
            !prompt.contains("\"typed_text\""),
            "prompt must not include raw typed text field, got: {prompt}"
        );
        assert!(
            !prompt.contains("\"text\":"),
            "prompt must not include raw text field, got: {prompt}"
        );
    }

    #[test]
    fn encode_callout_attachment_produces_matching_descriptor_for_2x3_rgba() {
        let image = rgba_image_2x3();

        let (descriptor, png) = encode_callout_attachment(&image).expect("encoding succeeds");

        assert_eq!(descriptor.width, 2);
        assert_eq!(descriptor.height, 3);
        assert!(matches!(descriptor.media_type, MediaType::Png));
        assert_eq!(
            descriptor.byte_count,
            u64::try_from(png.len()).expect("png fits in u64")
        );
        // PNG signature is the first 8 bytes 0x89 0x50 0x4E 0x47 0x0D 0x0A 0x1A 0x0A.
        assert_eq!(&png[..8], &[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A]);
    }

    #[test]
    fn build_callout_prompt_step_struct_serializes_all_fields() {
        let payload = CalloutPromptStep {
            index: 3,
            source: 42,
            title: "Scroll".to_string(),
            caption: "The list scrolls to reveal more rows.".to_string(),
            kind: "Scroll".to_string(),
        };
        let json = serde_json::to_string(&payload).expect("serialize");
        assert!(json.contains("\"index\":3"));
        assert!(json.contains("\"source\":42"));
        assert!(json.contains("\"title\":\"Scroll\""));
        assert!(json.contains("\"caption\":\"The list scrolls to reveal more rows.\""));
        assert!(json.contains("\"kind\":\"Scroll\""));
    }
}
