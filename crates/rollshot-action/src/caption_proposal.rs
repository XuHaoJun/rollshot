use crate::guide::Guide;
use crate::models::{CandidateId, FrameId, GuideStep};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct CaptionProposalId(pub u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct CaptionSuggestionId(pub u64);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CaptionProposalProvenance {
    Agent { run_id: u64 },
}

#[derive(Debug, Clone, PartialEq)]
pub struct CaptionSuggestionDraft {
    pub step_source: CandidateId,
    pub title: Option<String>,
    pub caption: String,
    pub confidence: f32,
    pub rationale: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaptionSuggestionBase {
    pub source: CandidateId,
    pub index: usize,
    pub title: String,
    pub caption: String,
    pub keyframe: FrameId,
}

impl CaptionSuggestionBase {
    fn from_step(step: &GuideStep) -> Self {
        Self {
            source: step.source,
            index: step.index,
            title: step.title.clone(),
            caption: step.caption.clone(),
            keyframe: step.keyframe,
        }
    }

    fn matches_step(&self, step: &GuideStep) -> bool {
        self.source == step.source
            && self.title == step.title
            && self.caption == step.caption
            && self.keyframe == step.keyframe
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaptionSuggestionStatus {
    Pending,
    Accepted,
    Rejected,
    Stale,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CaptionSuggestion {
    pub id: CaptionSuggestionId,
    pub base: CaptionSuggestionBase,
    pub suggested_title: Option<String>,
    pub suggested_caption: String,
    pub confidence: f32,
    pub rationale: Option<String>,
    pub provenance: CaptionProposalProvenance,
    pub status: CaptionSuggestionStatus,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CaptionProposal {
    pub id: CaptionProposalId,
    pub provenance: CaptionProposalProvenance,
    pub suggestions: Vec<CaptionSuggestion>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaptionApplyOutcome {
    Applied,
    Missing,
    Stale,
    NotPending,
}

impl CaptionProposal {
    pub fn from_agent_drafts(
        id: CaptionProposalId,
        run_id: u64,
        guide: &Guide,
        drafts: Vec<CaptionSuggestionDraft>,
    ) -> Self {
        let provenance = CaptionProposalProvenance::Agent { run_id };
        let mut suggestions = Vec::new();
        for draft in drafts {
            let Some(step) = guide
                .steps()
                .iter()
                .find(|step| step.source == draft.step_source)
            else {
                continue;
            };
            let suggested_caption = draft.caption.trim().to_string();
            if suggested_caption.is_empty() {
                continue;
            }
            suggestions.push(CaptionSuggestion {
                id: CaptionSuggestionId(suggestions.len() as u64 + 1),
                base: CaptionSuggestionBase::from_step(step),
                suggested_title: draft.title.filter(|title| !title.trim().is_empty()),
                suggested_caption,
                confidence: draft.confidence.clamp(0.0, 1.0),
                rationale: draft
                    .rationale
                    .and_then(|text| (!text.trim().is_empty()).then(|| text.trim().to_string())),
                provenance: provenance.clone(),
                status: CaptionSuggestionStatus::Pending,
            });
        }

        Self {
            id,
            provenance,
            suggestions,
        }
    }

    pub fn apply(&mut self, guide: &mut Guide, id: CaptionSuggestionId) -> CaptionApplyOutcome {
        let Some(suggestion) = self.suggestions.iter_mut().find(|s| s.id == id) else {
            return CaptionApplyOutcome::Missing;
        };
        if suggestion.status != CaptionSuggestionStatus::Pending {
            return CaptionApplyOutcome::NotPending;
        }
        let Some(step) = guide
            .steps()
            .iter()
            .find(|step| step.source == suggestion.base.source)
            .cloned()
        else {
            suggestion.status = CaptionSuggestionStatus::Stale;
            return CaptionApplyOutcome::Stale;
        };
        if !suggestion.base.matches_step(&step) {
            suggestion.status = CaptionSuggestionStatus::Stale;
            return CaptionApplyOutcome::Stale;
        }

        let title = suggestion
            .suggested_title
            .clone()
            .unwrap_or_else(|| step.title.clone());
        if guide.set_title_and_caption(step.index, title, suggestion.suggested_caption.clone()) {
            suggestion.status = CaptionSuggestionStatus::Accepted;
            CaptionApplyOutcome::Applied
        } else {
            suggestion.status = CaptionSuggestionStatus::Stale;
            CaptionApplyOutcome::Stale
        }
    }

    pub fn reject(&mut self, id: CaptionSuggestionId) -> bool {
        let Some(suggestion) = self.suggestions.iter_mut().find(|s| s.id == id) else {
            return false;
        };
        if suggestion.status != CaptionSuggestionStatus::Pending {
            return false;
        }
        suggestion.status = CaptionSuggestionStatus::Rejected;
        true
    }

    pub fn apply_all(&mut self, guide: &mut Guide) -> Vec<CaptionApplyOutcome> {
        let ids: Vec<_> = self
            .suggestions
            .iter()
            .filter(|suggestion| suggestion.status == CaptionSuggestionStatus::Pending)
            .map(|suggestion| suggestion.id)
            .collect();
        ids.into_iter().map(|id| self.apply(guide, id)).collect()
    }

    pub fn has_pending(&self) -> bool {
        self.suggestions
            .iter()
            .any(|suggestion| suggestion.status == CaptionSuggestionStatus::Pending)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::guide::Guide;
    use crate::models::{CandidateKind, CandidateStep, DetectReason};

    fn guide() -> Guide {
        Guide::from_candidates(vec![
            CandidateStep {
                id: 10,
                kind: CandidateKind::Click,
                reason: DetectReason::ClickConfirmed,
                at_ms: 120,
                keyframe: 1,
                nearby: vec![1],
            },
            CandidateStep {
                id: 11,
                kind: CandidateKind::Typing,
                reason: DetectReason::TypingSettled,
                at_ms: 340,
                keyframe: 2,
                nearby: vec![2],
            },
        ])
    }

    #[test]
    fn builds_proposal_from_agent_drafts() {
        let guide = guide();
        let proposal = CaptionProposal::from_agent_drafts(
            CaptionProposalId(7),
            42,
            &guide,
            vec![CaptionSuggestionDraft {
                step_source: 10,
                title: Some("Open Settings".to_string()),
                caption: "The user opens settings from the toolbar.".to_string(),
                confidence: 0.82,
                rationale: Some("Click step starts the settings flow.".to_string()),
            }],
        );

        assert_eq!(proposal.id, CaptionProposalId(7));
        assert_eq!(proposal.suggestions.len(), 1);
        assert_eq!(proposal.suggestions[0].base.source, 10);
        assert_eq!(
            proposal.suggestions[0].suggested_title.as_deref(),
            Some("Open Settings")
        );
        assert_eq!(
            proposal.suggestions[0].provenance,
            CaptionProposalProvenance::Agent { run_id: 42 }
        );
        assert_eq!(
            proposal.suggestions[0].status,
            CaptionSuggestionStatus::Pending
        );
    }

    #[test]
    fn accepting_pending_suggestion_updates_title_and_caption() {
        let mut guide = guide();
        let mut proposal = CaptionProposal::from_agent_drafts(
            CaptionProposalId(1),
            42,
            &guide,
            vec![CaptionSuggestionDraft {
                step_source: 10,
                title: Some("Open Settings".to_string()),
                caption: "The settings panel appears.".to_string(),
                confidence: 0.7,
                rationale: None,
            }],
        );

        let outcome = proposal.apply(&mut guide, CaptionSuggestionId(1));

        assert_eq!(outcome, CaptionApplyOutcome::Applied);
        let step = guide.steps().iter().find(|step| step.source == 10).unwrap();
        assert_eq!(step.title, "Open Settings");
        assert_eq!(step.caption, "The settings panel appears.");
        assert_eq!(
            proposal.suggestions[0].status,
            CaptionSuggestionStatus::Accepted
        );
    }

    #[test]
    fn accepting_stale_suggestion_does_not_mutate_guide() {
        let mut guide = guide();
        let mut proposal = CaptionProposal::from_agent_drafts(
            CaptionProposalId(1),
            42,
            &guide,
            vec![CaptionSuggestionDraft {
                step_source: 10,
                title: Some("Open Settings".to_string()),
                caption: "The settings panel appears.".to_string(),
                confidence: 0.7,
                rationale: None,
            }],
        );
        assert!(guide.rename(1, "Manual title".to_string()));

        let outcome = proposal.apply(&mut guide, CaptionSuggestionId(1));

        assert_eq!(outcome, CaptionApplyOutcome::Stale);
        let step = guide.steps().iter().find(|step| step.source == 10).unwrap();
        assert_eq!(step.title, "Manual title");
        assert_eq!(step.caption, "");
        assert_eq!(
            proposal.suggestions[0].status,
            CaptionSuggestionStatus::Stale
        );
    }

    #[test]
    fn reject_marks_pending_suggestion_without_mutating_guide() {
        let mut guide = guide();
        let mut proposal = CaptionProposal::from_agent_drafts(
            CaptionProposalId(1),
            42,
            &guide,
            vec![CaptionSuggestionDraft {
                step_source: 10,
                title: None,
                caption: "The settings panel appears.".to_string(),
                confidence: 0.7,
                rationale: None,
            }],
        );

        assert!(proposal.reject(CaptionSuggestionId(1)));
        let outcome = proposal.apply(&mut guide, CaptionSuggestionId(1));

        assert_eq!(outcome, CaptionApplyOutcome::NotPending);
        assert_eq!(guide.steps()[0].caption, "");
        assert_eq!(
            proposal.suggestions[0].status,
            CaptionSuggestionStatus::Rejected
        );
    }

    #[test]
    fn construction_filters_unknown_sources_and_empty_captions_with_stable_ids() {
        let guide = guide();
        let proposal = CaptionProposal::from_agent_drafts(
            CaptionProposalId(1),
            42,
            &guide,
            vec![
                CaptionSuggestionDraft {
                    step_source: 999,
                    title: Some("Ignored".to_string()),
                    caption: "Unknown step.".to_string(),
                    confidence: 0.7,
                    rationale: None,
                },
                CaptionSuggestionDraft {
                    step_source: 10,
                    title: Some("Ignored".to_string()),
                    caption: "   ".to_string(),
                    confidence: 0.7,
                    rationale: None,
                },
                CaptionSuggestionDraft {
                    step_source: 11,
                    title: None,
                    caption: "The user enters information into the form.".to_string(),
                    confidence: 0.8,
                    rationale: None,
                },
            ],
        );

        assert_eq!(proposal.suggestions.len(), 1);
        assert_eq!(proposal.suggestions[0].id, CaptionSuggestionId(1));
        assert_eq!(proposal.suggestions[0].base.source, 11);
    }
}
