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
    VisualAnnotationProposalOrigin, VisualAnnotationSuggestionDraft, VisualAnnotationSuggestionId,
};
use rollshot_agent::domain::{AttachmentDescriptor, AuthorizedModelInput, MediaType};
use rollshot_agent::driver::{AgentConfig, AgentRunner, VisualAnnotationProfile};
use rollshot_agent::runtime::RunCancellation;
use rollshot_agent::skills::bundled_action_guide_visual_annotations_use;
use rollshot_agent::{ProviderAdapter, VisualAnnotationDraft, VisualAnnotationRunTerminal};
use rollshot_image_document::{ImagePoint, ImageRect};
use sha2::{Digest, Sha256};

// ========================================================================
// Visual content digests
// ========================================================================

/// Compute a deterministic SHA-256 digest of the keyframe image.
///
/// Domain-separated with `rollshot-action-guide-keyframe-v1\0`, then width
/// (little-endian u32), height (little-endian u32), and the raw RGBA pixel
/// bytes. The digest binds the exact unflattened source pixels without
/// depending on any encoder output.
pub(crate) fn visual_keyframe_digest(image: &image::RgbaImage) -> [u8; 32] {
    let mut hash = Sha256::new();
    hash.update(b"rollshot-action-guide-keyframe-v1\0");
    hash.update(image.width().to_le_bytes());
    hash.update(image.height().to_le_bytes());
    hash.update(image.as_raw());
    hash.finalize().into()
}

/// Compute a deterministic SHA-256 digest of the annotation state.
///
/// Domain-separated with `rollshot-action-guide-annotations-v1\0`, then
/// `serde_json::to_vec` of the ordered, validated persisted annotation list.
/// No pixels, paths, or explanations enter the digest.
pub(crate) fn visual_annotation_state_digest(
    annotations: &[rollshot_image_document::Annotation],
) -> Result<[u8; 32], String> {
    let bytes = serde_json::to_vec(annotations)
        .map_err(|error| format!("serialize visual annotation state: {error}"))?;
    let mut hash = Sha256::new();
    hash.update(b"rollshot-action-guide-annotations-v1\0");
    hash.update(bytes);
    Ok(hash.finalize().into())
}

// ========================================================================
// Two-stage durable visual annotation dispatch types
// ========================================================================

/// Request to prepare visual annotation context. Captured when the workspace
/// starts a visual annotation suggestion and passed to the async preparation
/// worker.
pub(crate) enum VisualAnnotationContextRequest {
    /// Durable: saved project root exists and is clean.
    Durable {
        root: std::path::PathBuf,
        expected_revision: u64,
        step_source: u64,
        keyframe: u64,
    },
    /// Ephemeral: unsaved or dirty workspace; no durable identity.
    Ephemeral {
        guide: rollshot_action::Guide,
        step_source: u64,
        keyframe: u64,
    },
}

/// Prepared visual annotation context returned by the preparation worker.
/// Carries the digest values and origin needed to build source bindings
/// and launch the provider request.
#[derive(Debug)]
pub(crate) enum PreparedVisualAnnotationContext {
    Durable {
        guide: rollshot_action::Guide,
        projection: rollshot_action::project::ActionGuideContextProjectionV1,
        origin: VisualAnnotationProposalOrigin,
        project_root: std::path::PathBuf,
        step_source: u64,
        keyframe: u64,
    },
    Ephemeral {
        guide: rollshot_action::Guide,
        origin: VisualAnnotationProposalOrigin,
        step_source: u64,
        keyframe: u64,
    },
}

/// Async preparation worker for two-stage durable visual annotation dispatch.
///
/// For durable input, loads the project from disk via [`spawn_blocking`],
/// verifies the expected revision, and builds an
/// [`ActionGuideContextProjectionV1`]. For ephemeral input, computes
/// the guide digest (reusing the caption agent's algorithm) for provenance.
pub(crate) async fn prepare_visual_annotation_context_task(
    _run_id: u64,
    request: VisualAnnotationContextRequest,
) -> Result<PreparedVisualAnnotationContext, String> {
    match request {
        VisualAnnotationContextRequest::Durable {
            root,
            expected_revision,
            step_source,
            keyframe,
        } => {
            let root_for_load = root.clone();
            let loaded = tokio::task::spawn_blocking(move || {
                rollshot_action::project::load_project(&root_for_load)
            })
            .await
            .map_err(|_| "Project load task panicked.".to_string())?
            .map_err(|e| e.to_string())?;

            if loaded.manifest.revision != expected_revision {
                return Err(format!(
                    "Project was modified externally (expected revision {expected_revision}, got {}).",
                    loaded.manifest.revision
                ));
            }

            let projection =
                rollshot_action::project::ActionGuideContextProjectionV1::from_loaded_project(
                    &loaded,
                )
                .map_err(|e| format!("Visual annotation context projection failed: {e}"))?;
            let guide = projection
                .to_guide()
                .map_err(|e| format!("Guide from projection failed: {e}"))?;
            let origin = VisualAnnotationProposalOrigin::DurableProject {
                revision: projection.revision(),
                projection_digest: projection.digest().to_owned(),
            };

            tracing::info!(
                target: "rollshot::action::visual_annotation_agent",
                _run_id,
                revision = projection.revision(),
                step_count = guide.steps().len(),
                "durable visual annotation context prepared"
            );

            Ok(PreparedVisualAnnotationContext::Durable {
                guide,
                projection,
                origin,
                project_root: root,
                step_source,
                keyframe,
            })
        }
        VisualAnnotationContextRequest::Ephemeral {
            guide,
            step_source,
            keyframe,
        } => {
            let guide_digest =
                crate::timeline_workspace::caption_agent::compute_guide_digest(&guide);
            let origin = VisualAnnotationProposalOrigin::EphemeralGuide { guide_digest };
            Ok(PreparedVisualAnnotationContext::Ephemeral {
                guide,
                origin,
                step_source,
                keyframe,
            })
        }
    }
}

/// Inputs the workspace hands to the async visual annotation task. The image
/// is the original retained keyframe (cloned, not borrowed) so the `'static`
/// task can outlive the workspace borrow.
pub(crate) struct VisualAnnotationTaskInput {
    pub run_id: u64,
    pub origin: VisualAnnotationProposalOrigin,
    pub step: GuideStep,
    pub document_state_id: u64,
    pub image: image::RgbaImage,
    pub keyframe_sha256: [u8; 32],
    pub annotation_state_sha256: [u8; 32],
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
    origin: VisualAnnotationProposalOrigin,
    step: &GuideStep,
    document_state_id: u64,
    image_width: u32,
    image_height: u32,
    keyframe_sha256: [u8; 32],
    annotation_state_sha256: [u8; 32],
    agent_drafts: Vec<VisualAnnotationDraft>,
) -> Result<VisualAnnotationProposal, rollshot_action::VisualAnnotationProposalError> {
    let drafts: Vec<VisualAnnotationSuggestionDraft> = agent_drafts
        .into_iter()
        .map(|d| suggestion_to_draft(d, image_width, image_height))
        .collect();
    VisualAnnotationProposal::from_agent_drafts(
        VisualAnnotationProposalId(run_id),
        run_id,
        origin,
        step,
        document_state_id,
        image_width,
        image_height,
        keyframe_sha256,
        annotation_state_sha256,
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
        origin,
        step,
        document_state_id,
        image,
        keyframe_sha256,
        annotation_state_sha256,
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
    let skill = match bundled_action_guide_visual_annotations_use() {
        Some(s) => s,
        None => {
            tracing::error!(
                target: "rollshot::action::visual_annotation_agent",
                "bundled visual annotations skill failed to load"
            );
            return Ok(VisualAnnotationTaskResult::NoSuggestion {
                reason: Some("skill unavailable".to_owned()),
            });
        }
    };
    let profile = match VisualAnnotationProfile::from_skill(&skill) {
        Ok(p) => p,
        Err(e) => {
            tracing::error!(
                target: "rollshot::action::visual_annotation_agent",
                error = ?e,
                "visual annotation profile rejected the bundled skill"
            );
            return Ok(VisualAnnotationTaskResult::NoSuggestion {
                reason: Some("skill unavailable".to_owned()),
            });
        }
    };
    let terminal = runner
        .run_visual_annotation_with_provider(
            profile,
            authorized,
            &*adapter,
            rollshot_agent::visual_annotation_run_budget(),
            &cancellation,
        )
        .await;
    Ok(map_terminal_to_result(
        terminal,
        run_id,
        origin,
        &step,
        document_state_id,
        image_width,
        image_height,
        keyframe_sha256,
        annotation_state_sha256,
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
    origin: VisualAnnotationProposalOrigin,
    step: &GuideStep,
    document_state_id: u64,
    image_width: u32,
    image_height: u32,
    keyframe_sha256: [u8; 32],
    annotation_state_sha256: [u8; 32],
) -> VisualAnnotationTaskResult {
    match terminal {
        VisualAnnotationRunTerminal::Suggested(drafts) => {
            match suggestion_batch_to_proposal(
                run_id,
                origin,
                step,
                document_state_id,
                image_width,
                image_height,
                keyframe_sha256,
                annotation_state_sha256,
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
        VisualAnnotationRunTerminal::AuthorityDenied { operation } => {
            tracing::warn!(
                target: "rollshot::action::visual_annotation_agent",
                run_id,
                operation = ?operation,
                "visual annotation authority denied"
            );
            VisualAnnotationTaskResult::NoSuggestion {
                reason: Some("Visual annotation model did not return a usable suggestion.".to_string()),
            }
        }
        VisualAnnotationRunTerminal::AuditFailure { category } => {
            tracing::warn!(
                target: "rollshot::action::visual_annotation_agent",
                run_id,
                category = ?category,
                "visual annotation audit failure"
            );
            VisualAnnotationTaskResult::NoSuggestion {
                reason: Some("Visual annotation model did not return a usable suggestion.".to_string()),
            }
        }
    }
}

// ========================================================================
// Visual source binding and authority
// ========================================================================

/// Build a [`SourceBinding`] for a visual annotation run.
///
/// Durable contexts bind to `ActionGuideVisualAnnotationProject` when a
/// project root is available, otherwise fall back to the ephemeral variant.
/// Ephemeral contexts always bind to
/// `ActionGuideVisualAnnotationEphemeralGuide`.
pub(crate) fn visual_source_binding(
    context: &PreparedVisualAnnotationContext,
    step_source: u64,
    keyframe: u64,
    keyframe_sha256: [u8; 32],
    annotation_state_sha256: [u8; 32],
) -> rollshot_agent::product_task::SourceBinding {
    use rollshot_agent::product_task::SourceBinding;
    match context {
        PreparedVisualAnnotationContext::Durable {
            projection,
            project_root,
            ..
        } => SourceBinding::ActionGuideVisualAnnotationProject {
            project_root_sha256: crate::timeline_workspace::caption_agent::project_root_digest(
                project_root,
            ),
            revision: projection.revision(),
            projection_digest: projection.digest().to_owned(),
            step_source,
            keyframe,
            keyframe_sha256,
            annotation_state_sha256,
        },
        PreparedVisualAnnotationContext::Ephemeral { origin, .. } => {
            let guide_digest = match origin {
                VisualAnnotationProposalOrigin::EphemeralGuide { guide_digest } => {
                    guide_digest.clone()
                }
                _ => unreachable!("ephemeral context always has EphemeralGuide origin"),
            };
            SourceBinding::ActionGuideVisualAnnotationEphemeralGuide {
                guide_digest,
                step_source,
                keyframe,
                keyframe_sha256,
                annotation_state_sha256,
            }
        }
    }
}

/// Build an [`AuthoritySnapshot`] for a visual annotation run.
///
/// Always grants [`RunOperation::DiscloseScreenshotAttachment`] and
/// [`RunOperation::SubmitReviewCandidate`] with
/// [`DisclosureCeiling::FullScreenshot`]. The caller supplies the
/// [`AuthoritySubject`].
pub(crate) fn visual_authority(
    task_id: rollshot_agent::product_task::ProductTaskId,
    run_id: rollshot_agent::domain::RunId,
    subject: rollshot_agent::authority::AuthoritySubject,
) -> Result<rollshot_agent::authority::AuthoritySnapshot, String> {
    use rollshot_agent::authority::{
        AuthorityBinding, AuthoritySnapshot, DisclosureCeiling, RunOperation,
    };
    use rollshot_agent::product_task::TaskAttemptId;

    let mut grants = std::collections::BTreeSet::new();
    grants.insert(RunOperation::DiscloseScreenshotAttachment);
    grants.insert(RunOperation::SubmitReviewCandidate);
    let binding = AuthorityBinding::new(task_id, TaskAttemptId::new(1), run_id, subject);
    AuthoritySnapshot::new(
        binding,
        "rollshot-v1".to_owned(),
        DisclosureCeiling::FullScreenshot,
        true,
        std::collections::BTreeSet::new(),
        grants,
    )
    .map_err(|e| format!("build visual authority: {e}"))
}

#[cfg(test)]
fn visual_authority_with_grants(
    task_id: rollshot_agent::product_task::ProductTaskId,
    run_id: rollshot_agent::domain::RunId,
    subject: rollshot_agent::authority::AuthoritySubject,
    grant_disclose: bool,
    grant_submit: bool,
) -> Result<rollshot_agent::authority::AuthoritySnapshot, String> {
    use rollshot_agent::authority::{
        AuthorityBinding, AuthoritySnapshot, DisclosureCeiling, RunOperation,
    };
    use rollshot_agent::product_task::TaskAttemptId;

    let mut grants = std::collections::BTreeSet::new();
    if grant_disclose {
        grants.insert(RunOperation::DiscloseScreenshotAttachment);
    }
    if grant_submit {
        grants.insert(RunOperation::SubmitReviewCandidate);
    }
    let binding = AuthorityBinding::new(task_id, TaskAttemptId::new(1), run_id, subject);
    AuthoritySnapshot::new(
        binding,
        "rollshot-v1".to_owned(),
        DisclosureCeiling::FullScreenshot,
        true,
        std::collections::BTreeSet::new(),
        grants,
    )
    .map_err(|e| format!("build visual authority: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{Rgba, RgbaImage};
    use rollshot_action::{CandidateKind, DetectReason};
    use rollshot_agent::audit::AuditFailureCategory;
    use rollshot_agent::authority::RunOperation;
    use rollshot_agent::NormalizedPoint;
    use rollshot_agent::NormalizedRect;
    use std::sync::Arc;

    fn test_origin() -> VisualAnnotationProposalOrigin {
        VisualAnnotationProposalOrigin::EphemeralGuide {
            guide_digest: "aa".repeat(32),
        }
    }

    fn test_keyframe_sha() -> [u8; 32] {
        [1u8; 32]
    }

    fn test_annotation_sha() -> [u8; 32] {
        [2u8; 32]
    }

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
        let proposal = suggestion_batch_to_proposal(7, test_origin(), &step(), 12, 400, 200, test_keyframe_sha(), test_annotation_sha(), agent_batch())
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
            suggestion_batch_to_proposal(1, test_origin(), &step(), 1, 400, 200, test_keyframe_sha(), test_annotation_sha(), agent_batch()).expect("proposal");
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
            suggestion_batch_to_proposal(1, test_origin(), &step(), 1, 400, 200, test_keyframe_sha(), test_annotation_sha(), agent_batch()).expect("proposal");
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
            suggestion_batch_to_proposal(1, test_origin(), &step(), 1, 400, 200, test_keyframe_sha(), test_annotation_sha(), agent_batch()).expect("proposal");
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
            suggestion_batch_to_proposal(3, test_origin(), &step(), 5, 800, 600, test_keyframe_sha(), test_annotation_sha(), single).expect("proposal");
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

    #[test]
    fn visual_annotation_user_prompt_baseline_is_exact() {
        assert_eq!(
            build_visual_annotation_prompt(&step()),
            "Inspect this reviewed Action Guide step and suggest visual annotation overlays \
             (Number Callout, Text Note, or Opaque Redaction) on the attached keyframe. \
             Prefer calling the submit_visual_annotation_suggestions tool. If tool calling \
             is unavailable, return only JSON in the same schema. The image is the only \
             source of truth. Use the step metadata as context only. \
             Step source=10, keyframe=7, title=\"Open Settings\"",
        );
    }

    #[test]
    fn terminal_budget_exhausted_maps_to_no_suggestion_with_reason() {
        use rollshot_agent::runtime::BudgetDimension;
        let result = map_terminal_to_result(
            VisualAnnotationRunTerminal::BudgetExhausted {
                dimension: BudgetDimension::WallTime,
            },
            7,
            test_origin(),
            &step(),
            0,
            100,
            80,
            test_keyframe_sha(),
            test_annotation_sha(),
        );
        let VisualAnnotationTaskResult::NoSuggestion { reason } = result else {
            panic!("expected no-suggestion result");
        };
        assert_eq!(
            reason.as_deref(),
            Some("Visual annotation suggestion budget exhausted."),
        );
    }

    #[test]
    fn terminal_provider_failure_maps_to_no_suggestion_with_reason() {
        let result = map_terminal_to_result(
            VisualAnnotationRunTerminal::ProviderFailure,
            7,
            test_origin(),
            &step(),
            0,
            100,
            80,
            test_keyframe_sha(),
            test_annotation_sha(),
        );
        let VisualAnnotationTaskResult::NoSuggestion { reason } = result else {
            panic!("expected no-suggestion result");
        };
        assert_eq!(
            reason.as_deref(),
            Some("Visual annotation provider failed."),
        );
    }

    #[test]
    fn terminal_protocol_failure_maps_to_no_suggestion_with_reason() {
        let result = map_terminal_to_result(
            VisualAnnotationRunTerminal::ProtocolFailure,
            7,
            test_origin(),
            &step(),
            0,
            100,
            80,
            test_keyframe_sha(),
            test_annotation_sha(),
        );
        let VisualAnnotationTaskResult::NoSuggestion { reason } = result else {
            panic!("expected no-suggestion result");
        };
        assert_eq!(
            reason.as_deref(),
            Some("Visual annotation model did not return a usable suggestion."),
        );
    }

    #[test]
    fn terminal_cancelled_maps_to_no_suggestion_without_reason() {
        let result = map_terminal_to_result(
            VisualAnnotationRunTerminal::Cancelled,
            7,
            test_origin(),
            &step(),
            0,
            100,
            80,
            test_keyframe_sha(),
            test_annotation_sha(),
        );
        let VisualAnnotationTaskResult::NoSuggestion { reason } = result else {
            panic!("expected no-suggestion result");
        };
        assert!(reason.is_none());
    }

    #[test]
    fn terminal_no_suggestion_maps_to_no_suggestion_without_reason() {
        let result = map_terminal_to_result(
            VisualAnnotationRunTerminal::NoSuggestion(
                rollshot_agent::VisualAnnotationNoSuggestion::NoClearTarget {
                    reason: Some("model declined".to_string()),
                },
            ),
            7,
            test_origin(),
            &step(),
            0,
            100,
            80,
            test_keyframe_sha(),
            test_annotation_sha(),
        );
        let VisualAnnotationTaskResult::NoSuggestion { reason } = result else {
            panic!("expected no-suggestion result");
        };
        assert!(reason.is_none());
    }

    // ------------------------------------------------------------------
    // Visual content digest tests
    // ------------------------------------------------------------------

    fn annotation_fixture(id: u64) -> rollshot_image_document::Annotation {
        rollshot_image_document::Annotation::OpaqueRedaction {
            id: rollshot_image_document::AnnotationId(id),
            bounds: rollshot_image_document::ImageRect {
                x: 0.0,
                y: 0.0,
                width: 100.0,
                height: 100.0,
            },
        }
    }

    fn digest_hex(bytes: [u8; 32]) -> String {
        bytes.iter().map(|byte| format!("{byte:02x}")).collect()
    }

    #[test]
    fn visual_keyframe_digest_is_domain_separated_and_dimension_sensitive() {
        let one = RgbaImage::from_pixel(2, 1, Rgba([1, 2, 3, 255]));
        let two = RgbaImage::from_pixel(1, 2, Rgba([1, 2, 3, 255]));
        assert_ne!(visual_keyframe_digest(&one), visual_keyframe_digest(&two));
        assert_eq!(visual_keyframe_digest(&one), visual_keyframe_digest(&one));
    }

    #[test]
    fn annotation_digest_is_order_and_content_sensitive() {
        let a = vec![annotation_fixture(1), annotation_fixture(2)];
        let b = vec![annotation_fixture(2), annotation_fixture(1)];
        assert_ne!(
            visual_annotation_state_digest(&a).unwrap(),
            visual_annotation_state_digest(&b).unwrap(),
        );
    }

    #[test]
    fn visual_content_digest_vectors_are_stable() {
        let image = RgbaImage::from_pixel(1, 1, Rgba([1, 2, 3, 255]));
        assert_eq!(
            digest_hex(visual_keyframe_digest(&image)),
            "076499b61e7fac624835f05426686bf725b0220d24f5b2c18d2d70368ac2cbef",
        );
        assert_eq!(
            digest_hex(visual_annotation_state_digest(&[]).unwrap()),
            "c2f1bf7391acf52d4af9a694e2e4253e3fc9eafb11aaf105d8a8b1e2ffed8fd2",
        );
    }

    // ------------------------------------------------------------------
    // Context preparation drift tests
    // ------------------------------------------------------------------

    fn run<F: std::future::Future>(future: F) -> F::Output {
        tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .build()
            .unwrap()
            .block_on(future)
    }

    /// Durable preparation rejects a project whose revision has changed since
    /// the user opened the consent dialog.
    #[test]
    fn durable_preparation_rejects_changed_revision() {
        use rollshot_action::project::{
            create_project, load_project, EnabledOutputs, ProjectSnapshot, ProjectStep,
            ProjectStepId, SnapshotFrame, SnapshotFramePayload,
        };

        let dir = tempfile::tempdir().unwrap();
        let project_dir = dir.path().join("guide.rollshot-guide");
        let steps = vec![ProjectStep {
            id: ProjectStepId(1),
            order: 1,
            title: "Step 1".into(),
            caption: Some("Caption 1".into()),
            kind: rollshot_action::CandidateKind::Click,
            reason: rollshot_action::DetectReason::ClickConfirmed,
            at_ms: 100,
            keyframe: 1,
            nearby: vec![1],
            annotations: None,
        }];
        let snapshot = ProjectSnapshot {
            base_revision: None,
            title: "Test Guide".into(),
            capture_region: rollshot_action::CaptureRegion {
                x: 0,
                y: 0,
                width: 8,
                height: 8,
            },
            input_source: rollshot_action::InputSourceKind::VisualOnly,
            input_capability: rollshot_action::InputCapability::SemanticEvents,
            enabled_outputs: EnabledOutputs::default(),
            frames: vec![SnapshotFrame {
                id: 1,
                at_ms: 100,
                payload: SnapshotFramePayload::Pixels(Arc::new(image::RgbaImage::new(8, 8))),
            }],
            steps,
            import_warnings: Vec::new(),
        };
        create_project(&snapshot, &project_dir).unwrap();
        let loaded = load_project(&project_dir).unwrap();
        let actual_revision = loaded.manifest.revision;

        let result = run(prepare_visual_annotation_context_task(
            42,
            VisualAnnotationContextRequest::Durable {
                root: project_dir.clone(),
                expected_revision: actual_revision + 999,
                step_source: 1,
                keyframe: 1,
            },
        ));
        let err = result.unwrap_err();
        assert!(
            err.contains("modified externally"),
            "expected revision mismatch error, got: {err}"
        );
    }

    /// Ephemeral preparation produces an origin with a guide digest and
    /// carries no filesystem path.
    #[test]
    fn ephemeral_preparation_never_carries_path() {
        use rollshot_action::CandidateKind;

        let step = rollshot_action::GuideStep {
            index: 1,
            title: "Open".into(),
            caption: "Done".into(),
            kind: CandidateKind::Click,
            reason: rollshot_action::DetectReason::ClickConfirmed,
            at_ms: 100,
            keyframe: 5,
            nearby: vec![],
            source: 3,
        };
        let guide = rollshot_action::Guide::from_reviewed_steps("Test".into(), vec![step]).unwrap();

        let ctx = run(prepare_visual_annotation_context_task(
            99,
            VisualAnnotationContextRequest::Ephemeral {
                guide: guide.clone(),
                step_source: 3,
                keyframe: 5,
            },
        ))
        .unwrap();

        match ctx {
            PreparedVisualAnnotationContext::Ephemeral {
                origin,
                step_source,
                keyframe,
                ..
            } => {
                match origin {
                    VisualAnnotationProposalOrigin::EphemeralGuide { guide_digest } => {
                        assert!(!guide_digest.is_empty(), "guide digest must be populated");
                        // The digest should be the same as the caption agent's.
                        let expected_digest = crate::timeline_workspace::caption_agent::compute_guide_digest(&guide);
                        assert_eq!(guide_digest, expected_digest);
                    }
                    other => panic!("expected EphemeralGuide origin, got {other:?}"),
                }
                assert_eq!(step_source, 3);
                assert_eq!(keyframe, 5);
            }
            other => panic!("expected Ephemeral context, got {other:?}"),
        }
    }

    #[test]
    fn authority_denied_maps_to_no_suggestion() {
        let terminal = VisualAnnotationRunTerminal::AuthorityDenied {
            operation: RunOperation::DiscloseScreenshotAttachment,
        };
        let result = map_terminal_to_result(
            terminal,
            1,
            test_origin(),
            &step(),
            5,
            400,
            200,
            test_keyframe_sha(),
            test_annotation_sha(),
        );
        assert!(matches!(
            result,
            VisualAnnotationTaskResult::NoSuggestion { reason: Some(_) }
        ));
    }

    #[test]
    fn audit_failure_maps_to_no_suggestion() {
        let terminal = VisualAnnotationRunTerminal::AuditFailure {
            category: AuditFailureCategory::AppendPreCommitFailure,
        };
        let result = map_terminal_to_result(
            terminal,
            1,
            test_origin(),
            &step(),
            5,
            400,
            200,
            test_keyframe_sha(),
            test_annotation_sha(),
        );
        assert!(matches!(
            result,
            VisualAnnotationTaskResult::NoSuggestion { reason: Some(_) }
        ));
    }

    // ------------------------------------------------------------------
    // Visual source binding tests
    // ------------------------------------------------------------------

    fn task_id() -> rollshot_agent::product_task::ProductTaskId {
        rollshot_agent::product_task::ProductTaskId::parse(
            "task-00000000-0000-4000-8000-000000000099",
        )
        .unwrap()
    }

    fn run_id() -> rollshot_agent::domain::RunId {
        rollshot_agent::domain::RunId::parse("run-00000000-0000-4000-8000-000000000099").unwrap()
    }

    fn document_binding() -> rollshot_agent::product_task::DocumentContentBinding {
        let state = rollshot_agent::product_task::AnnotationStateV1 {
            width: 100,
            height: 80,
            state_id: 5,
            annotations: vec![],
        };
        rollshot_agent::product_task::DocumentContentBinding::new(
            test_keyframe_sha(),
            &state,
            5,
        )
        .unwrap()
    }

    /// Create a durable prepared context for binding tests.
    fn durable_context() -> PreparedVisualAnnotationContext {
        use rollshot_action::project::{
            create_project, EnabledOutputs, ProjectSnapshot, ProjectStep,
            ProjectStepId, SnapshotFrame, SnapshotFramePayload,
        };

        let dir = tempfile::tempdir().unwrap();
        let project_dir = dir.path().join("guide.rollshot-guide");
        let steps = vec![ProjectStep {
            id: ProjectStepId(1),
            order: 1,
            title: "Step 1".into(),
            caption: Some("Caption 1".into()),
            kind: CandidateKind::Click,
            reason: DetectReason::ClickConfirmed,
            at_ms: 100,
            keyframe: 1,
            nearby: vec![1],
            annotations: None,
        }];
        let snapshot = ProjectSnapshot {
            base_revision: None,
            title: "Test Guide".into(),
            capture_region: rollshot_action::CaptureRegion {
                x: 0,
                y: 0,
                width: 8,
                height: 8,
            },
            input_source: rollshot_action::InputSourceKind::VisualOnly,
            input_capability: rollshot_action::InputCapability::SemanticEvents,
            enabled_outputs: EnabledOutputs::default(),
            frames: vec![SnapshotFrame {
                id: 1,
                at_ms: 100,
                payload: SnapshotFramePayload::Pixels(Arc::new(image::RgbaImage::new(8, 8))),
            }],
            steps,
            import_warnings: Vec::new(),
        };
        create_project(&snapshot, &project_dir).unwrap();

        let ctx = run(prepare_visual_annotation_context_task(
            42,
            VisualAnnotationContextRequest::Durable {
                root: project_dir,
                expected_revision: 1,
                step_source: 3,
                keyframe: 1,
            },
        ))
        .unwrap();
        // Leak the tempdir so the project root stays valid for the test.
        std::mem::forget(dir);
        ctx
    }

    /// Create an ephemeral prepared context for binding tests.
    fn ephemeral_context() -> PreparedVisualAnnotationContext {
        let step = rollshot_action::GuideStep {
            index: 1,
            title: "Open".into(),
            caption: "Done".into(),
            kind: CandidateKind::Click,
            reason: DetectReason::ClickConfirmed,
            at_ms: 100,
            keyframe: 5,
            nearby: vec![],
            source: 3,
        };
        let guide =
            rollshot_action::Guide::from_reviewed_steps("Test".into(), vec![step]).unwrap();
        run(prepare_visual_annotation_context_task(
            99,
            VisualAnnotationContextRequest::Ephemeral {
                guide,
                step_source: 3,
                keyframe: 5,
            },
        ))
        .unwrap()
    }

    #[test]
    fn visual_durable_binding_has_all_fields() {
        use rollshot_agent::product_task::SourceBinding;

        let ctx = durable_context();
        // Extract the project root stored in the durable context.
        let root = match &ctx {
            PreparedVisualAnnotationContext::Durable { project_root, .. } => {
                project_root.clone()
            }
            _ => panic!("expected Durable context"),
        };
        let binding = visual_source_binding(
            &ctx,
            3,
            1,
            test_keyframe_sha(),
            test_annotation_sha(),
        );
        match binding {
            SourceBinding::ActionGuideVisualAnnotationProject {
                project_root_sha256,
                revision,
                projection_digest,
                step_source,
                keyframe,
                keyframe_sha256,
                annotation_state_sha256,
            } => {
                assert_eq!(
                    project_root_sha256,
                    crate::timeline_workspace::caption_agent::project_root_digest(&root),
                    "project_root_sha256 must match the digest of the project root stored in the context",
                );
                assert_eq!(revision, 1);
                assert!(!projection_digest.is_empty());
                assert_eq!(step_source, 3);
                assert_eq!(keyframe, 1);
                assert_eq!(keyframe_sha256, test_keyframe_sha());
                assert_eq!(annotation_state_sha256, test_annotation_sha());
            }
            other => panic!(
                "expected ActionGuideVisualAnnotationProject, got {other:?}"
            ),
        }
    }

    #[test]
    fn visual_ephemeral_binding_has_all_fields() {
        use rollshot_agent::product_task::SourceBinding;

        let ctx = ephemeral_context();
        let binding = visual_source_binding(
            &ctx,
            3,
            5,
            test_keyframe_sha(),
            test_annotation_sha(),
        );
        match binding {
            SourceBinding::ActionGuideVisualAnnotationEphemeralGuide {
                guide_digest,
                step_source,
                keyframe,
                keyframe_sha256,
                annotation_state_sha256,
            } => {
                assert!(!guide_digest.is_empty());
                assert_eq!(step_source, 3);
                assert_eq!(keyframe, 5);
                assert_eq!(keyframe_sha256, test_keyframe_sha());
                assert_eq!(annotation_state_sha256, test_annotation_sha());
            }
            other => panic!(
                "expected ActionGuideVisualAnnotationEphemeralGuide, got {other:?}"
            ),
        }
    }

    #[test]
    fn visual_and_caption_bindings_never_identity_match() {
        use rollshot_agent::product_task::SourceBinding;

        // Same project root used for both a caption and a visual binding.
        let root = std::path::Path::new("/tmp/test-project");
        let caption = SourceBinding::ActionGuideProject {
            project_root_sha256: crate::timeline_workspace::caption_agent::project_root_digest(root),
            revision: 1,
            projection_digest: "ab".repeat(32),
        };
        let visual = SourceBinding::ActionGuideVisualAnnotationProject {
            project_root_sha256: crate::timeline_workspace::caption_agent::project_root_digest(root),
            revision: 1,
            projection_digest: "ab".repeat(32),
            step_source: 3,
            keyframe: 1,
            keyframe_sha256: test_keyframe_sha(),
            annotation_state_sha256: test_annotation_sha(),
        };
        assert!(
            !caption.identity_matches(&visual),
            "caption and visual bindings with the same root must never identity-match"
        );
    }

    // ------------------------------------------------------------------
    // Visual authority tests
    // ------------------------------------------------------------------

    #[test]
    fn visual_authority_grants_only_attachment_and_submit() {
        use rollshot_agent::authority::AuthoritySubject;

        let authority = visual_authority(
            task_id(),
            run_id(),
            AuthoritySubject::Document(document_binding()),
        )
        .unwrap();
        assert_eq!(
            authority.disclosure(),
            rollshot_agent::authority::DisclosureCeiling::FullScreenshot,
        );
        assert!(authority
            .authorize_tool(
                authority.run_id(),
                &AuthoritySubject::Document(document_binding()),
                RunOperation::DiscloseScreenshotAttachment,
            )
            .is_ok());
        assert!(authority
            .authorize_tool(
                authority.run_id(),
                &AuthoritySubject::Document(document_binding()),
                RunOperation::SubmitReviewCandidate,
            )
            .is_ok());
        assert!(authority
            .authorize_tool(
                authority.run_id(),
                &AuthoritySubject::Document(document_binding()),
                RunOperation::InspectPreparedImage,
            )
            .is_err());
    }

    #[test]
    fn caption_authority_grants_only_submit_and_forbids_images() {
        use rollshot_agent::authority::AuthoritySubject;

        let authority = crate::timeline_workspace::caption_agent::caption_authority(
            task_id(),
            run_id(),
            AuthoritySubject::Document(document_binding()),
        )
        .unwrap();
        assert_eq!(
            authority.disclosure(),
            rollshot_agent::authority::DisclosureCeiling::TextMetadataOnly,
        );
        assert!(authority
            .authorize_tool(
                authority.run_id(),
                &AuthoritySubject::Document(document_binding()),
                RunOperation::SubmitReviewCandidate,
            )
            .is_ok());
        assert!(authority
            .authorize_tool(
                authority.run_id(),
                &AuthoritySubject::Document(document_binding()),
                RunOperation::DiscloseScreenshotAttachment,
            )
            .is_err());
        assert!(authority
            .authorize_tool(
                authority.run_id(),
                &AuthoritySubject::Document(document_binding()),
                RunOperation::InspectPreparedImage,
            )
            .is_err());
    }
}
