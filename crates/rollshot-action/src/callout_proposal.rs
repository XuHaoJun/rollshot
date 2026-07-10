use crate::models::{CandidateId, FrameId, GuideStep};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct CalloutProposalId(pub u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct CalloutSuggestionId(pub u64);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CalloutProposalProvenance {
    Agent { run_id: u64 },
}

#[derive(Debug, Clone, PartialEq)]
pub struct CalloutSuggestionDraft {
    pub tip: rollshot_image_document::ImagePoint,
    pub confidence: f32,
    pub rationale: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CalloutSuggestionBase {
    pub step_source: CandidateId,
    pub keyframe: FrameId,
    pub document_state_id: u64,
    pub image_width: u32,
    pub image_height: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CalloutSuggestionStatus {
    Pending,
    Accepted,
    Rejected,
    Stale,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CalloutSuggestion {
    pub id: CalloutSuggestionId,
    pub base: CalloutSuggestionBase,
    pub tip: rollshot_image_document::ImagePoint,
    pub confidence: f32,
    pub rationale: Option<String>,
    pub provenance: CalloutProposalProvenance,
    pub status: CalloutSuggestionStatus,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CalloutProposal {
    pub id: CalloutProposalId,
    pub run_id: u64,
    pub origin: GuideStep,
    pub suggestion: CalloutSuggestion,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CalloutApplyOutcome {
    Ready,
    Missing,
    Stale,
    NotPending,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CalloutProposalError {
    #[error("callout tip must be finite")]
    NonFiniteTip,
    #[error("callout tip is outside the source image")]
    TipOutOfBounds,
    #[error("callout confidence must be finite and within 0..=1")]
    InvalidConfidence,
    #[error("callout rationale exceeds 500 characters")]
    RationaleTooLong,
}

const MAX_RATIONALE_CHARS: usize = 500;

impl CalloutProposal {
    pub fn from_agent_draft(
        id: CalloutProposalId,
        run_id: u64,
        step: &GuideStep,
        document_state_id: u64,
        image_width: u32,
        image_height: u32,
        draft: CalloutSuggestionDraft,
    ) -> Result<Self, CalloutProposalError> {
        if !draft.tip.is_finite() {
            return Err(CalloutProposalError::NonFiniteTip);
        }
        if draft.tip.x < 0.0
            || draft.tip.x >= image_width as f32
            || draft.tip.y < 0.0
            || draft.tip.y >= image_height as f32
        {
            return Err(CalloutProposalError::TipOutOfBounds);
        }
        if !draft.confidence.is_finite() || !(0.0..=1.0).contains(&draft.confidence) {
            return Err(CalloutProposalError::InvalidConfidence);
        }
        let rationale = match draft.rationale {
            Some(text) => {
                let trimmed = text.trim();
                if trimmed.is_empty() {
                    None
                } else if trimmed.chars().count() > MAX_RATIONALE_CHARS {
                    return Err(CalloutProposalError::RationaleTooLong);
                } else {
                    Some(trimmed.to_string())
                }
            }
            None => None,
        };

        let provenance = CalloutProposalProvenance::Agent { run_id };
        let base = CalloutSuggestionBase {
            step_source: step.source,
            keyframe: step.keyframe,
            document_state_id,
            image_width,
            image_height,
        };
        let suggestion = CalloutSuggestion {
            id: CalloutSuggestionId(1),
            base,
            tip: draft.tip,
            confidence: draft.confidence,
            rationale,
            provenance,
            status: CalloutSuggestionStatus::Pending,
        };

        Ok(Self {
            id,
            run_id,
            origin: step.clone(),
            suggestion,
        })
    }

    pub fn validate_acceptance(
        &mut self,
        step: Option<&GuideStep>,
        document_state_id: u64,
        image_width: u32,
        image_height: u32,
    ) -> CalloutApplyOutcome {
        if self.suggestion.status != CalloutSuggestionStatus::Pending {
            return CalloutApplyOutcome::NotPending;
        }
        let Some(current) = step else {
            self.suggestion.status = CalloutSuggestionStatus::Stale;
            return CalloutApplyOutcome::Missing;
        };
        let base = &self.suggestion.base;
        if base.step_source != current.source
            || base.keyframe != current.keyframe
            || base.document_state_id != document_state_id
            || base.image_width != image_width
            || base.image_height != image_height
        {
            self.suggestion.status = CalloutSuggestionStatus::Stale;
            return CalloutApplyOutcome::Stale;
        }
        CalloutApplyOutcome::Ready
    }

    pub fn mark_applied(&mut self) {
        if self.suggestion.status == CalloutSuggestionStatus::Pending {
            self.suggestion.status = CalloutSuggestionStatus::Accepted;
        }
    }

    pub fn reject(&mut self) -> bool {
        if self.suggestion.status != CalloutSuggestionStatus::Pending {
            return false;
        }
        self.suggestion.status = CalloutSuggestionStatus::Rejected;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::guide::Guide;
    use crate::models::{CandidateKind, CandidateStep, DetectReason};
    use rollshot_image_document::ImagePoint;

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

    fn draft(
        tip: ImagePoint,
        confidence: f32,
        rationale: Option<String>,
    ) -> CalloutSuggestionDraft {
        CalloutSuggestionDraft {
            tip,
            confidence,
            rationale,
        }
    }

    #[test]
    fn valid_in_bounds_tip_builds_pending_proposal() {
        let step = step();
        let result = CalloutProposal::from_agent_draft(
            CalloutProposalId(42),
            7,
            &step,
            100,
            200,
            150,
            draft(
                ImagePoint::new(10.0, 20.0),
                0.9,
                Some("A reasonable rationale.".into()),
            ),
        );
        let proposal = result.expect("valid proposal");
        assert_eq!(proposal.id, CalloutProposalId(42));
        assert_eq!(proposal.run_id, 7);
        assert_eq!(proposal.suggestion.tip, ImagePoint::new(10.0, 20.0));
        assert_eq!(proposal.suggestion.confidence, 0.9);
        assert_eq!(
            proposal.suggestion.rationale.as_deref(),
            Some("A reasonable rationale.")
        );
        assert_eq!(proposal.suggestion.status, CalloutSuggestionStatus::Pending);
    }

    #[test]
    fn non_finite_tip_is_rejected() {
        let step = step();
        let nan_result = CalloutProposal::from_agent_draft(
            CalloutProposalId(1),
            1,
            &step,
            1,
            100,
            100,
            draft(ImagePoint::new(f32::NAN, 10.0), 0.5, None),
        );
        assert_eq!(nan_result.unwrap_err(), CalloutProposalError::NonFiniteTip);

        let inf_result = CalloutProposal::from_agent_draft(
            CalloutProposalId(1),
            1,
            &step,
            1,
            100,
            100,
            draft(ImagePoint::new(10.0, f32::INFINITY), 0.5, None),
        );
        assert_eq!(inf_result.unwrap_err(), CalloutProposalError::NonFiniteTip);

        let neg_inf_result = CalloutProposal::from_agent_draft(
            CalloutProposalId(1),
            1,
            &step,
            1,
            100,
            100,
            draft(ImagePoint::new(f32::NEG_INFINITY, 10.0), 0.5, None),
        );
        assert_eq!(
            neg_inf_result.unwrap_err(),
            CalloutProposalError::NonFiniteTip
        );
    }

    #[test]
    fn tip_on_right_or_bottom_edge_is_out_of_bounds() {
        let step = step();
        let right_edge = CalloutProposal::from_agent_draft(
            CalloutProposalId(1),
            1,
            &step,
            1,
            100,
            100,
            draft(ImagePoint::new(100.0, 10.0), 0.5, None),
        );
        assert_eq!(
            right_edge.unwrap_err(),
            CalloutProposalError::TipOutOfBounds
        );

        let bottom_edge = CalloutProposal::from_agent_draft(
            CalloutProposalId(1),
            1,
            &step,
            1,
            100,
            100,
            draft(ImagePoint::new(10.0, 100.0), 0.5, None),
        );
        assert_eq!(
            bottom_edge.unwrap_err(),
            CalloutProposalError::TipOutOfBounds
        );
    }

    #[test]
    fn tip_at_origin_and_inner_pixel_is_in_bounds() {
        let step = step();
        let origin = CalloutProposal::from_agent_draft(
            CalloutProposalId(1),
            1,
            &step,
            1,
            100,
            100,
            draft(ImagePoint::new(0.0, 0.0), 0.5, None),
        );
        assert!(origin.is_ok());

        let just_inside = CalloutProposal::from_agent_draft(
            CalloutProposalId(1),
            1,
            &step,
            1,
            100,
            100,
            draft(ImagePoint::new(99.999, 99.999), 0.5, None),
        );
        assert!(just_inside.is_ok());
    }

    #[test]
    fn negative_or_oversized_tip_is_out_of_bounds() {
        let step = step();
        let negative = CalloutProposal::from_agent_draft(
            CalloutProposalId(1),
            1,
            &step,
            1,
            100,
            100,
            draft(ImagePoint::new(-0.1, 10.0), 0.5, None),
        );
        assert_eq!(negative.unwrap_err(), CalloutProposalError::TipOutOfBounds);

        let oversized = CalloutProposal::from_agent_draft(
            CalloutProposalId(1),
            1,
            &step,
            1,
            100,
            100,
            draft(ImagePoint::new(10.0, 200.0), 0.5, None),
        );
        assert_eq!(oversized.unwrap_err(), CalloutProposalError::TipOutOfBounds);
    }

    #[test]
    fn invalid_confidence_is_rejected() {
        let step = step();
        let low = CalloutProposal::from_agent_draft(
            CalloutProposalId(1),
            1,
            &step,
            1,
            100,
            100,
            draft(ImagePoint::new(10.0, 10.0), -0.1, None),
        );
        assert_eq!(low.unwrap_err(), CalloutProposalError::InvalidConfidence);

        let high = CalloutProposal::from_agent_draft(
            CalloutProposalId(1),
            1,
            &step,
            1,
            100,
            100,
            draft(ImagePoint::new(10.0, 10.0), 1.1, None),
        );
        assert_eq!(high.unwrap_err(), CalloutProposalError::InvalidConfidence);

        let nan = CalloutProposal::from_agent_draft(
            CalloutProposalId(1),
            1,
            &step,
            1,
            100,
            100,
            draft(ImagePoint::new(10.0, 10.0), f32::NAN, None),
        );
        assert_eq!(nan.unwrap_err(), CalloutProposalError::InvalidConfidence);
    }

    #[test]
    fn boundary_confidence_values_are_accepted() {
        let step = step();
        let zero = CalloutProposal::from_agent_draft(
            CalloutProposalId(1),
            1,
            &step,
            1,
            100,
            100,
            draft(ImagePoint::new(10.0, 10.0), 0.0, None),
        );
        assert!(zero.is_ok());

        let one = CalloutProposal::from_agent_draft(
            CalloutProposalId(1),
            1,
            &step,
            1,
            100,
            100,
            draft(ImagePoint::new(10.0, 10.0), 1.0, None),
        );
        assert!(one.is_ok());
    }

    #[test]
    fn oversized_rationale_is_rejected() {
        let step = step();
        let too_long = "a".repeat(501);
        let result = CalloutProposal::from_agent_draft(
            CalloutProposalId(1),
            1,
            &step,
            1,
            100,
            100,
            draft(ImagePoint::new(10.0, 10.0), 0.5, Some(too_long.clone())),
        );
        assert_eq!(result.unwrap_err(), CalloutProposalError::RationaleTooLong);
    }

    #[test]
    fn trimmed_rationale_is_stored_and_whitespace_only_is_dropped() {
        let step = step();
        let padded = CalloutProposal::from_agent_draft(
            CalloutProposalId(1),
            1,
            &step,
            1,
            100,
            100,
            draft(
                ImagePoint::new(10.0, 10.0),
                0.5,
                Some("   trimmed rationale   ".into()),
            ),
        )
        .expect("valid proposal");
        assert_eq!(
            padded.suggestion.rationale.as_deref(),
            Some("trimmed rationale")
        );

        let whitespace_only = CalloutProposal::from_agent_draft(
            CalloutProposalId(1),
            1,
            &step,
            1,
            100,
            100,
            draft(ImagePoint::new(10.0, 10.0), 0.5, Some("   \t  \n  ".into())),
        )
        .expect("valid proposal");
        assert_eq!(whitespace_only.suggestion.rationale, None);
    }

    #[test]
    fn rationale_length_is_measured_after_trimming() {
        let step = step();
        let padded_too_long = format!("   {}   ", "a".repeat(501));
        let result = CalloutProposal::from_agent_draft(
            CalloutProposalId(1),
            1,
            &step,
            1,
            100,
            100,
            draft(ImagePoint::new(10.0, 10.0), 0.5, Some(padded_too_long)),
        );
        assert_eq!(result.unwrap_err(), CalloutProposalError::RationaleTooLong);
    }

    #[test]
    fn exact_rationale_length_boundary_is_accepted() {
        let step = step();
        let exactly_500 = "a".repeat(500);
        let result = CalloutProposal::from_agent_draft(
            CalloutProposalId(1),
            1,
            &step,
            1,
            100,
            100,
            draft(ImagePoint::new(10.0, 10.0), 0.5, Some(exactly_500)),
        );
        assert!(result.is_ok());
    }

    #[test]
    fn agent_provenance_stores_supplied_fields_exactly() {
        let step = step();
        let proposal = CalloutProposal::from_agent_draft(
            CalloutProposalId(99),
            1234,
            &step,
            55,
            200,
            300,
            draft(ImagePoint::new(50.0, 60.0), 0.7, Some("note".into())),
        )
        .expect("valid proposal");

        assert_eq!(proposal.id, CalloutProposalId(99));
        assert_eq!(proposal.run_id, 1234);
        assert_eq!(proposal.suggestion.base.document_state_id, 55);
        assert_eq!(proposal.suggestion.base.image_width, 200);
        assert_eq!(proposal.suggestion.base.image_height, 300);
        assert_eq!(proposal.suggestion.base.step_source, step.source);
        assert_eq!(proposal.suggestion.base.keyframe, step.keyframe);
        assert_eq!(
            proposal.suggestion.provenance,
            CalloutProposalProvenance::Agent { run_id: 1234 }
        );
        assert_eq!(proposal.origin, step);
    }

    #[test]
    fn validate_acceptance_returns_ready_for_exact_match() {
        let step = step();
        let mut proposal = CalloutProposal::from_agent_draft(
            CalloutProposalId(1),
            1,
            &step,
            55,
            200,
            300,
            draft(ImagePoint::new(50.0, 60.0), 0.7, None),
        )
        .expect("valid proposal");

        let outcome = proposal.validate_acceptance(Some(&step), 55, 200, 300);
        assert_eq!(outcome, CalloutApplyOutcome::Ready);
        assert_eq!(proposal.suggestion.status, CalloutSuggestionStatus::Pending);
    }

    #[test]
    fn validate_acceptance_returns_missing_for_none_step() {
        let step = step();
        let mut proposal = CalloutProposal::from_agent_draft(
            CalloutProposalId(1),
            1,
            &step,
            55,
            200,
            300,
            draft(ImagePoint::new(50.0, 60.0), 0.7, None),
        )
        .expect("valid proposal");

        let outcome = proposal.validate_acceptance(None, 55, 200, 300);
        assert_eq!(outcome, CalloutApplyOutcome::Missing);
        assert_eq!(proposal.suggestion.status, CalloutSuggestionStatus::Stale);
    }

    #[test]
    fn validate_acceptance_returns_stale_for_changed_source() {
        let step = step();
        let mut proposal = CalloutProposal::from_agent_draft(
            CalloutProposalId(1),
            1,
            &step,
            55,
            200,
            300,
            draft(ImagePoint::new(50.0, 60.0), 0.7, None),
        )
        .expect("valid proposal");

        let mut changed = step.clone();
        changed.source = step.source + 1;
        let outcome = proposal.validate_acceptance(Some(&changed), 55, 200, 300);
        assert_eq!(outcome, CalloutApplyOutcome::Stale);
        assert_eq!(proposal.suggestion.status, CalloutSuggestionStatus::Stale);
    }

    #[test]
    fn validate_acceptance_returns_stale_for_replaced_keyframe() {
        let step = step();
        let mut proposal = CalloutProposal::from_agent_draft(
            CalloutProposalId(1),
            1,
            &step,
            55,
            200,
            300,
            draft(ImagePoint::new(50.0, 60.0), 0.7, None),
        )
        .expect("valid proposal");

        let mut changed = step.clone();
        changed.keyframe = step.keyframe + 1;
        let outcome = proposal.validate_acceptance(Some(&changed), 55, 200, 300);
        assert_eq!(outcome, CalloutApplyOutcome::Stale);
        assert_eq!(proposal.suggestion.status, CalloutSuggestionStatus::Stale);
    }

    #[test]
    fn validate_acceptance_returns_stale_for_different_state_id() {
        let step = step();
        let mut proposal = CalloutProposal::from_agent_draft(
            CalloutProposalId(1),
            1,
            &step,
            55,
            200,
            300,
            draft(ImagePoint::new(50.0, 60.0), 0.7, None),
        )
        .expect("valid proposal");

        let outcome = proposal.validate_acceptance(Some(&step), 56, 200, 300);
        assert_eq!(outcome, CalloutApplyOutcome::Stale);
        assert_eq!(proposal.suggestion.status, CalloutSuggestionStatus::Stale);
    }

    #[test]
    fn validate_acceptance_returns_stale_for_different_dimensions() {
        let step = step();
        let mut proposal = CalloutProposal::from_agent_draft(
            CalloutProposalId(1),
            1,
            &step,
            55,
            200,
            300,
            draft(ImagePoint::new(50.0, 60.0), 0.7, None),
        )
        .expect("valid proposal");

        let outcome = proposal.validate_acceptance(Some(&step), 55, 201, 300);
        assert_eq!(outcome, CalloutApplyOutcome::Stale);
        assert_eq!(proposal.suggestion.status, CalloutSuggestionStatus::Stale);

        let mut proposal2 = CalloutProposal::from_agent_draft(
            CalloutProposalId(1),
            1,
            &step,
            55,
            200,
            300,
            draft(ImagePoint::new(50.0, 60.0), 0.7, None),
        )
        .expect("valid proposal");

        let outcome = proposal2.validate_acceptance(Some(&step), 55, 200, 301);
        assert_eq!(outcome, CalloutApplyOutcome::Stale);
        assert_eq!(proposal2.suggestion.status, CalloutSuggestionStatus::Stale);
    }

    #[test]
    fn validate_acceptance_returns_not_pending_for_rejected_proposal() {
        let step = step();
        let mut proposal = CalloutProposal::from_agent_draft(
            CalloutProposalId(1),
            1,
            &step,
            55,
            200,
            300,
            draft(ImagePoint::new(50.0, 60.0), 0.7, None),
        )
        .expect("valid proposal");
        assert!(proposal.reject());

        let outcome = proposal.validate_acceptance(Some(&step), 55, 200, 300);
        assert_eq!(outcome, CalloutApplyOutcome::NotPending);
    }

    #[test]
    fn validate_acceptance_returns_not_pending_for_already_accepted_proposal() {
        let step = step();
        let mut proposal = CalloutProposal::from_agent_draft(
            CalloutProposalId(1),
            1,
            &step,
            55,
            200,
            300,
            draft(ImagePoint::new(50.0, 60.0), 0.7, None),
        )
        .expect("valid proposal");
        proposal.mark_applied();

        let outcome = proposal.validate_acceptance(Some(&step), 55, 200, 300);
        assert_eq!(outcome, CalloutApplyOutcome::NotPending);
    }

    #[test]
    fn restoring_captured_state_id_makes_base_match_again() {
        let step = step();
        let captured_state_id = 55_u64;
        let mut proposal = CalloutProposal::from_agent_draft(
            CalloutProposalId(1),
            1,
            &step,
            captured_state_id,
            200,
            300,
            draft(ImagePoint::new(50.0, 60.0), 0.7, None),
        )
        .expect("valid proposal");

        assert_eq!(
            proposal.suggestion.base.document_state_id,
            captured_state_id
        );

        let outcome = proposal.validate_acceptance(Some(&step), captured_state_id, 200, 300);
        assert_eq!(outcome, CalloutApplyOutcome::Ready);
    }

    #[test]
    fn mark_applied_transitions_pending_to_accepted_and_is_idempotent_after() {
        let step = step();
        let mut proposal = CalloutProposal::from_agent_draft(
            CalloutProposalId(1),
            1,
            &step,
            55,
            200,
            300,
            draft(ImagePoint::new(50.0, 60.0), 0.7, None),
        )
        .expect("valid proposal");
        proposal.mark_applied();
        assert_eq!(
            proposal.suggestion.status,
            CalloutSuggestionStatus::Accepted
        );
        let outcome_after = proposal.validate_acceptance(Some(&step), 55, 200, 300);
        assert_eq!(outcome_after, CalloutApplyOutcome::NotPending);
    }

    #[test]
    fn reject_returns_true_only_for_pending_proposal() {
        let step = step();
        let mut proposal = CalloutProposal::from_agent_draft(
            CalloutProposalId(1),
            1,
            &step,
            55,
            200,
            300,
            draft(ImagePoint::new(50.0, 60.0), 0.7, None),
        )
        .expect("valid proposal");

        assert!(proposal.reject());
        assert!(!proposal.reject());
        assert_eq!(
            proposal.suggestion.status,
            CalloutSuggestionStatus::Rejected
        );
    }

    #[test]
    fn error_display_messages_match_documented_strings() {
        assert_eq!(
            format!("{}", CalloutProposalError::NonFiniteTip),
            "callout tip must be finite"
        );
        assert_eq!(
            format!("{}", CalloutProposalError::TipOutOfBounds),
            "callout tip is outside the source image"
        );
        assert_eq!(
            format!("{}", CalloutProposalError::InvalidConfidence),
            "callout confidence must be finite and within 0..=1"
        );
        assert_eq!(
            format!("{}", CalloutProposalError::RationaleTooLong),
            "callout rationale exceeds 500 characters"
        );
    }
}
