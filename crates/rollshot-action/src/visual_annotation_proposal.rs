use crate::models::{CandidateId, FrameId, GuideStep};
use rollshot_image_document::{EditOp, ImagePoint, ImageRect};

pub const MAX_VISUAL_SUGGESTIONS: usize = 20;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct VisualAnnotationProposalId(pub u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct VisualAnnotationSuggestionId(pub u64);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VisualAnnotationProvenance {
    Agent { run_id: u64 },
}

#[derive(Debug, Clone, PartialEq)]
pub enum VisualAnnotationPayload {
    NumberCallout { tip: ImagePoint, bubble: ImagePoint },
    TextNote { position: ImagePoint, text: String },
    OpaqueRedaction { bounds: ImageRect },
}

#[derive(Debug, Clone, PartialEq)]
pub struct VisualAnnotationSuggestionDraft {
    pub id: VisualAnnotationSuggestionId,
    pub payload: VisualAnnotationPayload,
    pub confidence: f32,
    pub rationale: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VisualAnnotationBase {
    pub step_source: CandidateId,
    pub keyframe: FrameId,
    pub document_state_id: u64,
    pub image_width: u32,
    pub image_height: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VisualAnnotationSuggestionStatus {
    Pending,
    Accepted,
    Rejected,
    Stale,
}

#[derive(Debug, Clone, PartialEq)]
pub struct VisualAnnotationSuggestion {
    pub id: VisualAnnotationSuggestionId,
    pub base: VisualAnnotationBase,
    pub payload: VisualAnnotationPayload,
    pub confidence: f32,
    pub rationale: Option<String>,
    pub provenance: VisualAnnotationProvenance,
    pub status: VisualAnnotationSuggestionStatus,
}

#[derive(Debug, Clone, PartialEq)]
pub struct VisualAnnotationProposal {
    pub id: VisualAnnotationProposalId,
    pub run_id: u64,
    pub origin: GuideStep,
    pub suggestions: Vec<VisualAnnotationSuggestion>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VisualAnnotationApplyOutcome {
    Ready,
    Missing,
    Stale,
    NotPending,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum VisualAnnotationProposalError {
    #[error("number callout tip must be finite")]
    NonFiniteCalloutTip,
    #[error("number callout bubble must be finite")]
    NonFiniteCalloutBubble,
    #[error("number callout tip is outside the source image")]
    CalloutTipOutOfBounds,
    #[error("number callout bubble is outside the source image")]
    CalloutBubbleOutOfBounds,
    #[error("text note position must be finite")]
    NonFiniteNotePosition,
    #[error("text note position is outside the source image")]
    NotePositionOutOfBounds,
    #[error("text note text must not be empty")]
    EmptyNoteText,
    #[error("text note text exceeds 500 characters")]
    NoteTextTooLong,
    #[error("redaction bounds must be finite")]
    NonFiniteRedactionBounds,
    #[error("redaction bounds are outside the source image")]
    RedactionOutOfBounds,
    #[error("redaction bounds have zero area")]
    RedactionZeroArea,
    #[error("confidence must be finite and within 0..=1")]
    InvalidConfidence,
    #[error("rationale exceeds 500 characters")]
    RationaleTooLong,
    #[error("batch must not be empty")]
    EmptyBatch,
    #[error("duplicate suggestion id in batch")]
    DuplicateSuggestionId,
    #[error("batch exceeds maximum of {0} suggestions")]
    BatchTooLarge(usize),
    #[error("no suggestions are fully pending")]
    NotFullyPending,
}

const MAX_RATIONALE_CHARS: usize = 500;

impl VisualAnnotationProposal {
    pub fn from_agent_drafts(
        id: VisualAnnotationProposalId,
        run_id: u64,
        step: &GuideStep,
        document_state_id: u64,
        image_width: u32,
        image_height: u32,
        drafts: Vec<VisualAnnotationSuggestionDraft>,
    ) -> Result<Self, VisualAnnotationProposalError> {
        if drafts.is_empty() {
            return Err(VisualAnnotationProposalError::EmptyBatch);
        }
        if drafts.len() > MAX_VISUAL_SUGGESTIONS {
            return Err(VisualAnnotationProposalError::BatchTooLarge(
                MAX_VISUAL_SUGGESTIONS,
            ));
        }

        let mut seen_ids = std::collections::HashSet::new();
        for draft in &drafts {
            if !seen_ids.insert(draft.id) {
                return Err(VisualAnnotationProposalError::DuplicateSuggestionId);
            }
        }

        let provenance = VisualAnnotationProvenance::Agent { run_id };
        let base = VisualAnnotationBase {
            step_source: step.source,
            keyframe: step.keyframe,
            document_state_id,
            image_width,
            image_height,
        };

        let mut suggestions = Vec::with_capacity(drafts.len());
        for draft in drafts {
            validate_draft(&draft, image_width, image_height)?;

            let rationale = match draft.rationale {
                Some(text) => {
                    let trimmed = text.trim();
                    if trimmed.is_empty() {
                        None
                    } else if trimmed.chars().count() > MAX_RATIONALE_CHARS {
                        return Err(VisualAnnotationProposalError::RationaleTooLong);
                    } else {
                        Some(trimmed.to_string())
                    }
                }
                None => None,
            };

            suggestions.push(VisualAnnotationSuggestion {
                id: draft.id,
                base: base.clone(),
                payload: draft.payload,
                confidence: draft.confidence,
                rationale,
                provenance: provenance.clone(),
                status: VisualAnnotationSuggestionStatus::Pending,
            });
        }

        Ok(Self {
            id,
            run_id,
            origin: step.clone(),
            suggestions,
        })
    }

    pub fn validate_item(
        &mut self,
        id: VisualAnnotationSuggestionId,
        step: Option<&GuideStep>,
        document_state_id: u64,
        image_width: u32,
        image_height: u32,
    ) -> VisualAnnotationApplyOutcome {
        let Some(suggestion) = self.suggestions.iter_mut().find(|s| s.id == id) else {
            return VisualAnnotationApplyOutcome::Missing;
        };
        if suggestion.status != VisualAnnotationSuggestionStatus::Pending {
            return VisualAnnotationApplyOutcome::NotPending;
        }
        let Some(current) = step else {
            suggestion.status = VisualAnnotationSuggestionStatus::Stale;
            return VisualAnnotationApplyOutcome::Missing;
        };
        let base = &suggestion.base;
        if base.step_source != current.source
            || base.keyframe != current.keyframe
            || base.document_state_id != document_state_id
            || base.image_width != image_width
            || base.image_height != image_height
        {
            suggestion.status = VisualAnnotationSuggestionStatus::Stale;
            return VisualAnnotationApplyOutcome::Stale;
        }
        VisualAnnotationApplyOutcome::Ready
    }

    pub fn reject(&mut self, id: VisualAnnotationSuggestionId) -> bool {
        let Some(suggestion) = self.suggestions.iter_mut().find(|s| s.id == id) else {
            return false;
        };
        if suggestion.status != VisualAnnotationSuggestionStatus::Pending {
            return false;
        }
        suggestion.status = VisualAnnotationSuggestionStatus::Rejected;
        true
    }

    pub fn reject_all(&mut self) {
        for suggestion in &mut self.suggestions {
            if suggestion.status == VisualAnnotationSuggestionStatus::Pending {
                suggestion.status = VisualAnnotationSuggestionStatus::Rejected;
            }
        }
    }

    pub fn pending_edit_ops(&self) -> Result<Vec<EditOp>, VisualAnnotationProposalError> {
        let has_pending = self
            .suggestions
            .iter()
            .any(|s| s.status == VisualAnnotationSuggestionStatus::Pending);
        if !has_pending {
            return Err(VisualAnnotationProposalError::NotFullyPending);
        }

        Ok(self
            .suggestions
            .iter()
            .filter(|s| s.status == VisualAnnotationSuggestionStatus::Pending)
            .map(|s| suggestion_to_edit_op(&s.payload))
            .collect())
    }
}

fn validate_draft(
    draft: &VisualAnnotationSuggestionDraft,
    image_width: u32,
    image_height: u32,
) -> Result<(), VisualAnnotationProposalError> {
    if !draft.confidence.is_finite() || !(0.0..=1.0).contains(&draft.confidence) {
        return Err(VisualAnnotationProposalError::InvalidConfidence);
    }
    match &draft.payload {
        VisualAnnotationPayload::NumberCallout { tip, bubble } => {
            if !tip.is_finite() {
                return Err(VisualAnnotationProposalError::NonFiniteCalloutTip);
            }
            if tip.x < 0.0
                || tip.x >= image_width as f32
                || tip.y < 0.0
                || tip.y >= image_height as f32
            {
                return Err(VisualAnnotationProposalError::CalloutTipOutOfBounds);
            }
            if !bubble.is_finite() {
                return Err(VisualAnnotationProposalError::NonFiniteCalloutBubble);
            }
            if bubble.x < 0.0
                || bubble.x >= image_width as f32
                || bubble.y < 0.0
                || bubble.y >= image_height as f32
            {
                return Err(VisualAnnotationProposalError::CalloutBubbleOutOfBounds);
            }
        }
        VisualAnnotationPayload::TextNote { position, text } => {
            if !position.is_finite() {
                return Err(VisualAnnotationProposalError::NonFiniteNotePosition);
            }
            if position.x < 0.0
                || position.x >= image_width as f32
                || position.y < 0.0
                || position.y >= image_height as f32
            {
                return Err(VisualAnnotationProposalError::NotePositionOutOfBounds);
            }
            let trimmed = text.trim();
            if trimmed.is_empty() {
                return Err(VisualAnnotationProposalError::EmptyNoteText);
            }
            if trimmed.chars().count() > MAX_RATIONALE_CHARS {
                return Err(VisualAnnotationProposalError::NoteTextTooLong);
            }
        }
        VisualAnnotationPayload::OpaqueRedaction { bounds } => {
            if !bounds.is_finite() {
                return Err(VisualAnnotationProposalError::NonFiniteRedactionBounds);
            }
            if bounds.x < 0.0
                || bounds.y < 0.0
                || bounds.x + bounds.width > image_width as f32
                || bounds.y + bounds.height > image_height as f32
            {
                return Err(VisualAnnotationProposalError::RedactionOutOfBounds);
            }
            if bounds.is_empty() {
                return Err(VisualAnnotationProposalError::RedactionZeroArea);
            }
        }
    }
    Ok(())
}

fn suggestion_to_edit_op(payload: &VisualAnnotationPayload) -> EditOp {
    match payload {
        VisualAnnotationPayload::NumberCallout { tip, bubble } => EditOp::AddNumberCallout {
            tip: *tip,
            bubble: *bubble,
        },
        VisualAnnotationPayload::TextNote { position, text } => EditOp::AddTextNote {
            position: *position,
            text: text.clone(),
        },
        VisualAnnotationPayload::OpaqueRedaction { bounds } => {
            EditOp::AddRedaction { bounds: *bounds }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::guide::Guide;
    use crate::models::{CandidateKind, CandidateStep, DetectReason};

    fn guide() -> Guide {
        Guide::from_candidates(vec![CandidateStep {
            id: 10,
            kind: CandidateKind::Click,
            reason: DetectReason::ClickConfirmed,
            at_ms: 120,
            keyframe: 7,
            nearby: vec![6, 7, 8],
        }])
    }

    fn step() -> GuideStep {
        guide().steps()[0].clone()
    }

    fn draft_callout(
        id: u64,
        tip: (f32, f32),
        bubble: (f32, f32),
    ) -> VisualAnnotationSuggestionDraft {
        VisualAnnotationSuggestionDraft {
            id: VisualAnnotationSuggestionId(id),
            payload: VisualAnnotationPayload::NumberCallout {
                tip: ImagePoint::new(tip.0, tip.1),
                bubble: ImagePoint::new(bubble.0, bubble.1),
            },
            confidence: 0.9,
            rationale: None,
        }
    }

    fn draft_note(id: u64, pos: (f32, f32), text: &str) -> VisualAnnotationSuggestionDraft {
        VisualAnnotationSuggestionDraft {
            id: VisualAnnotationSuggestionId(id),
            payload: VisualAnnotationPayload::TextNote {
                position: ImagePoint::new(pos.0, pos.1),
                text: text.to_string(),
            },
            confidence: 0.8,
            rationale: None,
        }
    }

    fn draft_redaction(id: u64, x: f32, y: f32, w: f32, h: f32) -> VisualAnnotationSuggestionDraft {
        VisualAnnotationSuggestionDraft {
            id: VisualAnnotationSuggestionId(id),
            payload: VisualAnnotationPayload::OpaqueRedaction {
                bounds: ImageRect {
                    x,
                    y,
                    width: w,
                    height: h,
                },
            },
            confidence: 0.7,
            rationale: None,
        }
    }

    #[test]
    fn valid_three_primitive_batch_lowers_to_three_edit_ops() {
        let proposal = VisualAnnotationProposal::from_agent_drafts(
            VisualAnnotationProposalId(9),
            9,
            &step(),
            41,
            320,
            240,
            vec![
                draft_callout(1, (16.0, 20.0), (80.0, 30.0)),
                draft_note(2, (24.0, 40.0), "Click Save"),
                draft_redaction(3, 100.0, 50.0, 80.0, 30.0),
            ],
        )
        .expect("valid batch");
        assert_eq!(proposal.pending_edit_ops().unwrap().len(), 3);
    }

    #[test]
    fn non_finite_callout_tip_is_rejected() {
        let result = VisualAnnotationProposal::from_agent_drafts(
            VisualAnnotationProposalId(1),
            1,
            &step(),
            1,
            100,
            100,
            vec![VisualAnnotationSuggestionDraft {
                id: VisualAnnotationSuggestionId(1),
                payload: VisualAnnotationPayload::NumberCallout {
                    tip: ImagePoint::new(f32::NAN, 10.0),
                    bubble: ImagePoint::new(50.0, 50.0),
                },
                confidence: 0.5,
                rationale: None,
            }],
        );
        assert_eq!(
            result.unwrap_err(),
            VisualAnnotationProposalError::NonFiniteCalloutTip
        );
    }

    #[test]
    fn non_finite_callout_bubble_is_rejected() {
        let result = VisualAnnotationProposal::from_agent_drafts(
            VisualAnnotationProposalId(1),
            1,
            &step(),
            1,
            100,
            100,
            vec![VisualAnnotationSuggestionDraft {
                id: VisualAnnotationSuggestionId(1),
                payload: VisualAnnotationPayload::NumberCallout {
                    tip: ImagePoint::new(10.0, 10.0),
                    bubble: ImagePoint::new(f32::INFINITY, 50.0),
                },
                confidence: 0.5,
                rationale: None,
            }],
        );
        assert_eq!(
            result.unwrap_err(),
            VisualAnnotationProposalError::NonFiniteCalloutBubble
        );
    }

    #[test]
    fn out_of_bounds_callout_tip_is_rejected() {
        let result = VisualAnnotationProposal::from_agent_drafts(
            VisualAnnotationProposalId(1),
            1,
            &step(),
            1,
            100,
            100,
            vec![VisualAnnotationSuggestionDraft {
                id: VisualAnnotationSuggestionId(1),
                payload: VisualAnnotationPayload::NumberCallout {
                    tip: ImagePoint::new(100.0, 10.0),
                    bubble: ImagePoint::new(50.0, 50.0),
                },
                confidence: 0.5,
                rationale: None,
            }],
        );
        assert_eq!(
            result.unwrap_err(),
            VisualAnnotationProposalError::CalloutTipOutOfBounds
        );
    }

    #[test]
    fn out_of_bounds_callout_bubble_is_rejected() {
        let result = VisualAnnotationProposal::from_agent_drafts(
            VisualAnnotationProposalId(1),
            1,
            &step(),
            1,
            100,
            100,
            vec![VisualAnnotationSuggestionDraft {
                id: VisualAnnotationSuggestionId(1),
                payload: VisualAnnotationPayload::NumberCallout {
                    tip: ImagePoint::new(10.0, 10.0),
                    bubble: ImagePoint::new(10.0, 100.0),
                },
                confidence: 0.5,
                rationale: None,
            }],
        );
        assert_eq!(
            result.unwrap_err(),
            VisualAnnotationProposalError::CalloutBubbleOutOfBounds
        );
    }

    #[test]
    fn non_finite_note_position_is_rejected() {
        let result = VisualAnnotationProposal::from_agent_drafts(
            VisualAnnotationProposalId(1),
            1,
            &step(),
            1,
            100,
            100,
            vec![VisualAnnotationSuggestionDraft {
                id: VisualAnnotationSuggestionId(1),
                payload: VisualAnnotationPayload::TextNote {
                    position: ImagePoint::new(f32::NAN, 10.0),
                    text: "hello".into(),
                },
                confidence: 0.5,
                rationale: None,
            }],
        );
        assert_eq!(
            result.unwrap_err(),
            VisualAnnotationProposalError::NonFiniteNotePosition
        );
    }

    #[test]
    fn out_of_bounds_note_position_is_rejected() {
        let result = VisualAnnotationProposal::from_agent_drafts(
            VisualAnnotationProposalId(1),
            1,
            &step(),
            1,
            100,
            100,
            vec![VisualAnnotationSuggestionDraft {
                id: VisualAnnotationSuggestionId(1),
                payload: VisualAnnotationPayload::TextNote {
                    position: ImagePoint::new(-1.0, 10.0),
                    text: "hello".into(),
                },
                confidence: 0.5,
                rationale: None,
            }],
        );
        assert_eq!(
            result.unwrap_err(),
            VisualAnnotationProposalError::NotePositionOutOfBounds
        );
    }

    #[test]
    fn blank_note_text_is_rejected() {
        let result = VisualAnnotationProposal::from_agent_drafts(
            VisualAnnotationProposalId(1),
            1,
            &step(),
            1,
            100,
            100,
            vec![VisualAnnotationSuggestionDraft {
                id: VisualAnnotationSuggestionId(1),
                payload: VisualAnnotationPayload::TextNote {
                    position: ImagePoint::new(10.0, 10.0),
                    text: "   ".into(),
                },
                confidence: 0.5,
                rationale: None,
            }],
        );
        assert_eq!(
            result.unwrap_err(),
            VisualAnnotationProposalError::EmptyNoteText
        );
    }

    #[test]
    fn note_text_over_500_chars_is_rejected() {
        let result = VisualAnnotationProposal::from_agent_drafts(
            VisualAnnotationProposalId(1),
            1,
            &step(),
            1,
            100,
            100,
            vec![VisualAnnotationSuggestionDraft {
                id: VisualAnnotationSuggestionId(1),
                payload: VisualAnnotationPayload::TextNote {
                    position: ImagePoint::new(10.0, 10.0),
                    text: "a".repeat(501),
                },
                confidence: 0.5,
                rationale: None,
            }],
        );
        assert_eq!(
            result.unwrap_err(),
            VisualAnnotationProposalError::NoteTextTooLong
        );
    }

    #[test]
    fn zero_area_redaction_is_rejected() {
        let result = VisualAnnotationProposal::from_agent_drafts(
            VisualAnnotationProposalId(1),
            1,
            &step(),
            1,
            100,
            100,
            vec![VisualAnnotationSuggestionDraft {
                id: VisualAnnotationSuggestionId(1),
                payload: VisualAnnotationPayload::OpaqueRedaction {
                    bounds: ImageRect {
                        x: 10.0,
                        y: 10.0,
                        width: 0.5,
                        height: 10.0,
                    },
                },
                confidence: 0.5,
                rationale: None,
            }],
        );
        assert_eq!(
            result.unwrap_err(),
            VisualAnnotationProposalError::RedactionZeroArea
        );
    }

    #[test]
    fn out_of_bounds_redaction_is_rejected() {
        let result = VisualAnnotationProposal::from_agent_drafts(
            VisualAnnotationProposalId(1),
            1,
            &step(),
            1,
            100,
            100,
            vec![VisualAnnotationSuggestionDraft {
                id: VisualAnnotationSuggestionId(1),
                payload: VisualAnnotationPayload::OpaqueRedaction {
                    bounds: ImageRect {
                        x: 90.0,
                        y: 90.0,
                        width: 20.0,
                        height: 20.0,
                    },
                },
                confidence: 0.5,
                rationale: None,
            }],
        );
        assert_eq!(
            result.unwrap_err(),
            VisualAnnotationProposalError::RedactionOutOfBounds
        );
    }

    #[test]
    fn non_finite_redaction_bounds_is_rejected() {
        let result = VisualAnnotationProposal::from_agent_drafts(
            VisualAnnotationProposalId(1),
            1,
            &step(),
            1,
            100,
            100,
            vec![VisualAnnotationSuggestionDraft {
                id: VisualAnnotationSuggestionId(1),
                payload: VisualAnnotationPayload::OpaqueRedaction {
                    bounds: ImageRect {
                        x: f32::NAN,
                        y: 10.0,
                        width: 20.0,
                        height: 20.0,
                    },
                },
                confidence: 0.5,
                rationale: None,
            }],
        );
        assert_eq!(
            result.unwrap_err(),
            VisualAnnotationProposalError::NonFiniteRedactionBounds
        );
    }

    #[test]
    fn invalid_confidence_is_rejected() {
        let result = VisualAnnotationProposal::from_agent_drafts(
            VisualAnnotationProposalId(1),
            1,
            &step(),
            1,
            100,
            100,
            vec![VisualAnnotationSuggestionDraft {
                id: VisualAnnotationSuggestionId(1),
                payload: VisualAnnotationPayload::TextNote {
                    position: ImagePoint::new(10.0, 10.0),
                    text: "hello".into(),
                },
                confidence: 1.1,
                rationale: None,
            }],
        );
        assert_eq!(
            result.unwrap_err(),
            VisualAnnotationProposalError::InvalidConfidence
        );
    }

    #[test]
    fn duplicate_suggestion_ids_are_rejected() {
        let result = VisualAnnotationProposal::from_agent_drafts(
            VisualAnnotationProposalId(1),
            1,
            &step(),
            1,
            100,
            100,
            vec![
                draft_note(1, (10.0, 10.0), "first"),
                draft_note(1, (20.0, 20.0), "second"),
            ],
        );
        assert_eq!(
            result.unwrap_err(),
            VisualAnnotationProposalError::DuplicateSuggestionId
        );
    }

    #[test]
    fn batch_over_max_is_rejected() {
        let mut drafts = Vec::new();
        for i in 0..=MAX_VISUAL_SUGGESTIONS {
            drafts.push(draft_note((i + 1) as u64, (10.0, 10.0), "note"));
        }
        let result = VisualAnnotationProposal::from_agent_drafts(
            VisualAnnotationProposalId(1),
            1,
            &step(),
            1,
            100,
            100,
            drafts,
        );
        assert_eq!(
            result.unwrap_err(),
            VisualAnnotationProposalError::BatchTooLarge(MAX_VISUAL_SUGGESTIONS)
        );
    }

    #[test]
    fn single_invalid_item_rejects_entire_batch() {
        let result = VisualAnnotationProposal::from_agent_drafts(
            VisualAnnotationProposalId(1),
            1,
            &step(),
            1,
            100,
            100,
            vec![
                draft_note(1, (10.0, 10.0), "good"),
                draft_note(2, (10.0, 10.0), ""),
            ],
        );
        assert_eq!(
            result.unwrap_err(),
            VisualAnnotationProposalError::EmptyNoteText
        );
    }

    #[test]
    fn validate_item_returns_missing_for_none_step() {
        let mut proposal = VisualAnnotationProposal::from_agent_drafts(
            VisualAnnotationProposalId(1),
            1,
            &step(),
            55,
            200,
            300,
            vec![draft_callout(1, (10.0, 20.0), (50.0, 50.0))],
        )
        .expect("valid");

        let outcome = proposal.validate_item(VisualAnnotationSuggestionId(1), None, 55, 200, 300);
        assert_eq!(outcome, VisualAnnotationApplyOutcome::Missing);
        assert_eq!(
            proposal.suggestions[0].status,
            VisualAnnotationSuggestionStatus::Stale
        );
    }

    #[test]
    fn validate_item_returns_stale_for_changed_source() {
        let mut proposal = VisualAnnotationProposal::from_agent_drafts(
            VisualAnnotationProposalId(1),
            1,
            &step(),
            55,
            200,
            300,
            vec![draft_callout(1, (10.0, 20.0), (50.0, 50.0))],
        )
        .expect("valid");

        let mut changed = step();
        changed.source = step().source + 1;
        let outcome = proposal.validate_item(
            VisualAnnotationSuggestionId(1),
            Some(&changed),
            55,
            200,
            300,
        );
        assert_eq!(outcome, VisualAnnotationApplyOutcome::Stale);
        assert_eq!(
            proposal.suggestions[0].status,
            VisualAnnotationSuggestionStatus::Stale
        );
    }

    #[test]
    fn validate_item_returns_stale_for_changed_keyframe() {
        let mut proposal = VisualAnnotationProposal::from_agent_drafts(
            VisualAnnotationProposalId(1),
            1,
            &step(),
            55,
            200,
            300,
            vec![draft_callout(1, (10.0, 20.0), (50.0, 50.0))],
        )
        .expect("valid");

        let mut changed = step();
        changed.keyframe = step().keyframe + 1;
        let outcome = proposal.validate_item(
            VisualAnnotationSuggestionId(1),
            Some(&changed),
            55,
            200,
            300,
        );
        assert_eq!(outcome, VisualAnnotationApplyOutcome::Stale);
    }

    #[test]
    fn validate_item_returns_stale_for_changed_state_id() {
        let mut proposal = VisualAnnotationProposal::from_agent_drafts(
            VisualAnnotationProposalId(1),
            1,
            &step(),
            55,
            200,
            300,
            vec![draft_callout(1, (10.0, 20.0), (50.0, 50.0))],
        )
        .expect("valid");

        let outcome =
            proposal.validate_item(VisualAnnotationSuggestionId(1), Some(&step()), 56, 200, 300);
        assert_eq!(outcome, VisualAnnotationApplyOutcome::Stale);
    }

    #[test]
    fn validate_item_returns_stale_for_changed_dimensions() {
        let mut proposal = VisualAnnotationProposal::from_agent_drafts(
            VisualAnnotationProposalId(1),
            1,
            &step(),
            55,
            200,
            300,
            vec![draft_callout(1, (10.0, 20.0), (50.0, 50.0))],
        )
        .expect("valid");

        let outcome =
            proposal.validate_item(VisualAnnotationSuggestionId(1), Some(&step()), 55, 201, 300);
        assert_eq!(outcome, VisualAnnotationApplyOutcome::Stale);
    }

    #[test]
    fn validate_item_returns_ready_for_exact_match() {
        let mut proposal = VisualAnnotationProposal::from_agent_drafts(
            VisualAnnotationProposalId(1),
            1,
            &step(),
            55,
            200,
            300,
            vec![draft_callout(1, (10.0, 20.0), (50.0, 50.0))],
        )
        .expect("valid");

        let outcome =
            proposal.validate_item(VisualAnnotationSuggestionId(1), Some(&step()), 55, 200, 300);
        assert_eq!(outcome, VisualAnnotationApplyOutcome::Ready);
        assert_eq!(
            proposal.suggestions[0].status,
            VisualAnnotationSuggestionStatus::Pending
        );
    }

    #[test]
    fn validate_item_returns_not_pending_for_rejected() {
        let mut proposal = VisualAnnotationProposal::from_agent_drafts(
            VisualAnnotationProposalId(1),
            1,
            &step(),
            55,
            200,
            300,
            vec![draft_callout(1, (10.0, 20.0), (50.0, 50.0))],
        )
        .expect("valid");
        proposal.reject(VisualAnnotationSuggestionId(1));

        let outcome =
            proposal.validate_item(VisualAnnotationSuggestionId(1), Some(&step()), 55, 200, 300);
        assert_eq!(outcome, VisualAnnotationApplyOutcome::NotPending);
    }

    #[test]
    fn reject_returns_true_only_for_pending() {
        let mut proposal = VisualAnnotationProposal::from_agent_drafts(
            VisualAnnotationProposalId(1),
            1,
            &step(),
            1,
            100,
            100,
            vec![draft_callout(1, (10.0, 20.0), (50.0, 50.0))],
        )
        .expect("valid");

        assert!(proposal.reject(VisualAnnotationSuggestionId(1)));
        assert!(!proposal.reject(VisualAnnotationSuggestionId(1)));
        assert_eq!(
            proposal.suggestions[0].status,
            VisualAnnotationSuggestionStatus::Rejected
        );
    }

    #[test]
    fn pending_edit_ops_returns_not_fully_pending_after_reject() {
        let mut proposal = VisualAnnotationProposal::from_agent_drafts(
            VisualAnnotationProposalId(1),
            1,
            &step(),
            1,
            100,
            100,
            vec![draft_callout(1, (10.0, 20.0), (50.0, 50.0))],
        )
        .expect("valid");
        proposal.reject(VisualAnnotationSuggestionId(1));

        assert_eq!(
            proposal.pending_edit_ops(),
            Err(VisualAnnotationProposalError::NotFullyPending)
        );
    }

    #[test]
    fn pending_edit_ops_returns_no_ops_after_reject_in_multi_batch() {
        let mut proposal = VisualAnnotationProposal::from_agent_drafts(
            VisualAnnotationProposalId(1),
            1,
            &step(),
            1,
            100,
            100,
            vec![
                draft_callout(1, (10.0, 20.0), (50.0, 50.0)),
                draft_note(2, (20.0, 30.0), "note"),
            ],
        )
        .expect("valid");
        proposal.reject(VisualAnnotationSuggestionId(1));

        let ops = proposal.pending_edit_ops().unwrap();
        assert_eq!(ops.len(), 1);
        assert!(matches!(ops[0], EditOp::AddTextNote { .. }));
    }

    #[test]
    fn pending_edit_ops_returns_not_fully_pending_after_stale() {
        let mut proposal = VisualAnnotationProposal::from_agent_drafts(
            VisualAnnotationProposalId(1),
            1,
            &step(),
            55,
            200,
            300,
            vec![draft_callout(1, (10.0, 20.0), (50.0, 50.0))],
        )
        .expect("valid");
        proposal.validate_item(VisualAnnotationSuggestionId(1), None, 55, 200, 300);

        assert_eq!(
            proposal.pending_edit_ops(),
            Err(VisualAnnotationProposalError::NotFullyPending)
        );
    }

    #[test]
    fn pending_edit_ops_maps_callout_to_add_number_callout() {
        let proposal = VisualAnnotationProposal::from_agent_drafts(
            VisualAnnotationProposalId(1),
            1,
            &step(),
            1,
            320,
            240,
            vec![draft_callout(1, (16.0, 20.0), (80.0, 30.0))],
        )
        .expect("valid");

        let ops = proposal.pending_edit_ops().unwrap();
        assert_eq!(ops.len(), 1);
        assert_eq!(
            ops[0],
            EditOp::AddNumberCallout {
                tip: ImagePoint::new(16.0, 20.0),
                bubble: ImagePoint::new(80.0, 30.0),
            }
        );
    }

    #[test]
    fn pending_edit_ops_maps_note_to_add_text_note() {
        let proposal = VisualAnnotationProposal::from_agent_drafts(
            VisualAnnotationProposalId(1),
            1,
            &step(),
            1,
            320,
            240,
            vec![draft_note(1, (24.0, 40.0), "Click Save")],
        )
        .expect("valid");

        let ops = proposal.pending_edit_ops().unwrap();
        assert_eq!(ops.len(), 1);
        assert_eq!(
            ops[0],
            EditOp::AddTextNote {
                position: ImagePoint::new(24.0, 40.0),
                text: "Click Save".to_string(),
            }
        );
    }

    #[test]
    fn pending_edit_ops_maps_redaction_to_add_redaction() {
        let proposal = VisualAnnotationProposal::from_agent_drafts(
            VisualAnnotationProposalId(1),
            1,
            &step(),
            1,
            320,
            240,
            vec![draft_redaction(1, 100.0, 50.0, 80.0, 30.0)],
        )
        .expect("valid");

        let ops = proposal.pending_edit_ops().unwrap();
        assert_eq!(ops.len(), 1);
        assert_eq!(
            ops[0],
            EditOp::AddRedaction {
                bounds: ImageRect {
                    x: 100.0,
                    y: 50.0,
                    width: 80.0,
                    height: 30.0,
                },
            }
        );
    }

    #[test]
    fn empty_batch_is_rejected() {
        let result = VisualAnnotationProposal::from_agent_drafts(
            VisualAnnotationProposalId(1),
            1,
            &step(),
            1,
            100,
            100,
            vec![],
        );
        assert_eq!(
            result.unwrap_err(),
            VisualAnnotationProposalError::EmptyBatch
        );
    }

    #[test]
    fn reject_all_marks_all_pending_as_rejected() {
        let mut proposal = VisualAnnotationProposal::from_agent_drafts(
            VisualAnnotationProposalId(1),
            1,
            &step(),
            1,
            100,
            100,
            vec![
                draft_callout(1, (10.0, 20.0), (50.0, 50.0)),
                draft_note(2, (20.0, 30.0), "note"),
            ],
        )
        .expect("valid");

        proposal.reject_all();

        assert!(proposal
            .suggestions
            .iter()
            .all(|s| s.status == VisualAnnotationSuggestionStatus::Rejected));
        assert_eq!(
            proposal.pending_edit_ops(),
            Err(VisualAnnotationProposalError::NotFullyPending)
        );
    }

    #[test]
    fn reject_nonexistent_id_returns_false() {
        let mut proposal = VisualAnnotationProposal::from_agent_drafts(
            VisualAnnotationProposalId(1),
            1,
            &step(),
            1,
            100,
            100,
            vec![draft_callout(1, (10.0, 20.0), (50.0, 50.0))],
        )
        .expect("valid");

        assert!(!proposal.reject(VisualAnnotationSuggestionId(99)));
    }

    #[test]
    fn error_display_messages_match_documented_strings() {
        assert_eq!(
            format!("{}", VisualAnnotationProposalError::NonFiniteCalloutTip),
            "number callout tip must be finite"
        );
        assert_eq!(
            format!("{}", VisualAnnotationProposalError::EmptyBatch),
            "batch must not be empty"
        );
        assert_eq!(
            format!("{}", VisualAnnotationProposalError::NotFullyPending),
            "no suggestions are fully pending"
        );
    }

    #[test]
    fn trimmed_rationale_is_stored() {
        let proposal = VisualAnnotationProposal::from_agent_drafts(
            VisualAnnotationProposalId(1),
            1,
            &step(),
            1,
            100,
            100,
            vec![VisualAnnotationSuggestionDraft {
                id: VisualAnnotationSuggestionId(1),
                payload: VisualAnnotationPayload::TextNote {
                    position: ImagePoint::new(10.0, 10.0),
                    text: "hello".into(),
                },
                confidence: 0.5,
                rationale: Some("  trimmed  ".into()),
            }],
        )
        .expect("valid");

        assert_eq!(
            proposal.suggestions[0].rationale.as_deref(),
            Some("trimmed")
        );
    }

    #[test]
    fn whitespace_only_rationale_is_dropped() {
        let proposal = VisualAnnotationProposal::from_agent_drafts(
            VisualAnnotationProposalId(1),
            1,
            &step(),
            1,
            100,
            100,
            vec![VisualAnnotationSuggestionDraft {
                id: VisualAnnotationSuggestionId(1),
                payload: VisualAnnotationPayload::TextNote {
                    position: ImagePoint::new(10.0, 10.0),
                    text: "hello".into(),
                },
                confidence: 0.5,
                rationale: Some("   \t  \n  ".into()),
            }],
        )
        .expect("valid");

        assert_eq!(proposal.suggestions[0].rationale, None);
    }

    #[test]
    fn all_suggestions_start_pending() {
        let proposal = VisualAnnotationProposal::from_agent_drafts(
            VisualAnnotationProposalId(1),
            1,
            &step(),
            1,
            100,
            100,
            vec![
                draft_callout(1, (10.0, 20.0), (50.0, 50.0)),
                draft_note(2, (20.0, 30.0), "note"),
                draft_redaction(3, 10.0, 10.0, 20.0, 20.0),
            ],
        )
        .expect("valid");

        assert!(proposal
            .suggestions
            .iter()
            .all(|s| s.status == VisualAnnotationSuggestionStatus::Pending));
    }
}
