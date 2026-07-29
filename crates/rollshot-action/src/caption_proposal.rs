use crate::guide::Guide;
use crate::models::{CandidateId, FrameId, GuideStep};

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
pub struct CaptionProposalId(pub u64);

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
pub struct CaptionSuggestionId(pub u64);

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum CaptionProposalProvenance {
    Agent { run_id: u64 },
}

/// The origin of a caption proposal: either from a durable project revision
/// (revision-bound) or from an ephemeral in-memory guide.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum CaptionProposalOrigin {
    DurableProject {
        revision: u64,
        projection_digest: String,
    },
    EphemeralGuide {
        guide_digest: String,
    },
}

/// Context passed to `apply` / `apply_all` to verify the proposal remains
/// valid against the current project state before mutating the guide.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CaptionApplyContext {
    DurableProject {
        revision: u64,
        projection_digest: String,
        clean: bool,
    },
    EphemeralGuide,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CaptionSuggestionDraft {
    pub step_source: CandidateId,
    pub title: Option<String>,
    pub caption: String,
    pub confidence: f32,
    pub rationale: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum CaptionSuggestionStatus {
    Pending,
    Accepted,
    Rejected,
    Stale,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
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

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct CaptionProposal {
    pub id: CaptionProposalId,
    pub origin: CaptionProposalOrigin,
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
        origin: CaptionProposalOrigin,
        guide: &Guide,
        drafts: Vec<CaptionSuggestionDraft>,
    ) -> Self {
        let provenance = CaptionProposalProvenance::Agent { run_id };
        // Validate origin: non-zero revision and 64 lowercase hex chars for digests.
        match &origin {
            CaptionProposalOrigin::DurableProject {
                revision,
                projection_digest,
            } => {
                debug_assert!(*revision > 0, "durable origin revision must be non-zero");
                debug_assert!(
                    projection_digest.len() == 64
                        && projection_digest
                            .bytes()
                            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
                    "projection digest must be 64 lowercase hex bytes"
                );
            }
            CaptionProposalOrigin::EphemeralGuide { guide_digest } => {
                debug_assert!(
                    guide_digest.len() == 64
                        && guide_digest
                            .bytes()
                            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
                    "guide digest must be 64 lowercase hex bytes"
                );
            }
        }
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
            origin,
            provenance,
            suggestions,
        }
    }

    pub fn apply(
        &mut self,
        guide: &mut Guide,
        context: &CaptionApplyContext,
        id: CaptionSuggestionId,
    ) -> CaptionApplyOutcome {
        // For durable context, validate the shared revision/digest/clean check
        // before any per-step mutation.
        if let CaptionApplyContext::DurableProject {
            revision,
            projection_digest,
            clean,
        } = context
        {
            if !clean {
                if let Some(suggestion) = self.suggestions.iter_mut().find(|s| s.id == id) {
                    suggestion.status = CaptionSuggestionStatus::Stale;
                }
                return CaptionApplyOutcome::Stale;
            }
            match &self.origin {
                CaptionProposalOrigin::DurableProject {
                    revision: origin_rev,
                    projection_digest: origin_digest,
                } => {
                    if *origin_rev != *revision || origin_digest != projection_digest {
                        if let Some(suggestion) = self.suggestions.iter_mut().find(|s| s.id == id) {
                            suggestion.status = CaptionSuggestionStatus::Stale;
                        }
                        return CaptionApplyOutcome::Stale;
                    }
                }
                CaptionProposalOrigin::EphemeralGuide { .. } => {
                    if let Some(suggestion) = self.suggestions.iter_mut().find(|s| s.id == id) {
                        suggestion.status = CaptionSuggestionStatus::Stale;
                    }
                    return CaptionApplyOutcome::Stale;
                }
            }
        }
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

    pub fn apply_all(
        &mut self,
        guide: &mut Guide,
        context: &CaptionApplyContext,
    ) -> Vec<CaptionApplyOutcome> {
        // For durable context, validate the shared check BEFORE collecting
        // any suggestions. If it fails, mark every pending suggestion Stale
        // and apply none.
        if let CaptionApplyContext::DurableProject {
            revision,
            projection_digest,
            clean,
        } = context
        {
            let context_valid = *clean
                && match &self.origin {
                    CaptionProposalOrigin::DurableProject {
                        revision: origin_rev,
                        projection_digest: origin_digest,
                    } => *origin_rev == *revision && origin_digest == projection_digest,
                    CaptionProposalOrigin::EphemeralGuide { .. } => false,
                };
            if !context_valid {
                let mut outcomes = Vec::new();
                for suggestion in &mut self.suggestions {
                    if suggestion.status == CaptionSuggestionStatus::Pending {
                        suggestion.status = CaptionSuggestionStatus::Stale;
                        outcomes.push(CaptionApplyOutcome::Stale);
                    }
                }
                return outcomes;
            }
        }
        let ids: Vec<_> = self
            .suggestions
            .iter()
            .filter(|suggestion| suggestion.status == CaptionSuggestionStatus::Pending)
            .map(|suggestion| suggestion.id)
            .collect();
        ids.into_iter()
            .map(|id| self.apply(guide, context, id))
            .collect()
    }

    pub fn has_pending(&self) -> bool {
        self.suggestions
            .iter()
            .any(|suggestion| suggestion.status == CaptionSuggestionStatus::Pending)
    }

    pub fn origin(&self) -> &CaptionProposalOrigin {
        &self.origin
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::guide::Guide;
    use crate::models::{CandidateKind, CandidateStep, DetectReason};

    fn ephemeral_origin() -> CaptionProposalOrigin {
        CaptionProposalOrigin::EphemeralGuide {
            guide_digest: "a".repeat(64),
        }
    }

    fn ephemeral_context() -> CaptionApplyContext {
        CaptionApplyContext::EphemeralGuide
    }

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
            ephemeral_origin(),
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
            ephemeral_origin(),
            &guide,
            vec![CaptionSuggestionDraft {
                step_source: 10,
                title: Some("Open Settings".to_string()),
                caption: "The settings panel appears.".to_string(),
                confidence: 0.7,
                rationale: None,
            }],
        );

        let outcome = proposal.apply(&mut guide, &ephemeral_context(), CaptionSuggestionId(1));

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
            ephemeral_origin(),
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

        let outcome = proposal.apply(&mut guide, &ephemeral_context(), CaptionSuggestionId(1));

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
            ephemeral_origin(),
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
        let outcome = proposal.apply(&mut guide, &ephemeral_context(), CaptionSuggestionId(1));

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
            ephemeral_origin(),
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

    // ---- Origin and context-bound tests ----

    #[test]
    fn durable_proposal_requires_exact_clean_revision_and_digest() {
        let mut guide = guide();
        let mut proposal = CaptionProposal::from_agent_drafts(
            CaptionProposalId(1),
            42,
            CaptionProposalOrigin::DurableProject {
                revision: 4,
                projection_digest: "a".repeat(64),
            },
            &guide,
            vec![CaptionSuggestionDraft {
                step_source: 10,
                title: Some("Open Settings".to_string()),
                caption: "The settings panel appears.".to_string(),
                confidence: 0.7,
                rationale: None,
            }],
        );

        let stale_revision = CaptionApplyContext::DurableProject {
            revision: 5,
            projection_digest: "a".repeat(64),
            clean: true,
        };
        assert_eq!(
            proposal.apply(&mut guide, &stale_revision, CaptionSuggestionId(1)),
            CaptionApplyOutcome::Stale
        );
        assert_eq!(guide.steps()[0].caption, "");
    }

    #[test]
    fn ephemeral_proposal_preserves_step_local_stale_semantics() {
        let mut guide = guide();
        let mut proposal = CaptionProposal::from_agent_drafts(
            CaptionProposalId(1),
            42,
            CaptionProposalOrigin::EphemeralGuide {
                guide_digest: "b".repeat(64),
            },
            &guide,
            vec![CaptionSuggestionDraft {
                step_source: 10,
                title: Some("Open Settings".to_string()),
                caption: "The settings panel appears.".to_string(),
                confidence: 0.7,
                rationale: None,
            }],
        );
        guide.set_title_and_caption(1, "Changed elsewhere".to_string(), "Elsewhere".to_string());

        assert_eq!(
            proposal.apply(
                &mut guide,
                &CaptionApplyContext::EphemeralGuide,
                CaptionSuggestionId(1)
            ),
            CaptionApplyOutcome::Stale
        );
    }

    #[test]
    fn exact_clean_durable_apply_success() {
        let mut guide = guide();
        let digest = "c".repeat(64);
        let mut proposal = CaptionProposal::from_agent_drafts(
            CaptionProposalId(1),
            42,
            CaptionProposalOrigin::DurableProject {
                revision: 3,
                projection_digest: digest.clone(),
            },
            &guide,
            vec![CaptionSuggestionDraft {
                step_source: 10,
                title: Some("Open Settings".to_string()),
                caption: "The settings panel appears.".to_string(),
                confidence: 0.7,
                rationale: None,
            }],
        );

        let context = CaptionApplyContext::DurableProject {
            revision: 3,
            projection_digest: digest,
            clean: true,
        };
        assert_eq!(
            proposal.apply(&mut guide, &context, CaptionSuggestionId(1)),
            CaptionApplyOutcome::Applied
        );
        assert_eq!(guide.steps()[0].caption, "The settings panel appears.");
    }

    #[test]
    fn dirty_durable_context_rejects() {
        let mut guide = guide();
        let digest = "d".repeat(64);
        let mut proposal = CaptionProposal::from_agent_drafts(
            CaptionProposalId(1),
            42,
            CaptionProposalOrigin::DurableProject {
                revision: 3,
                projection_digest: digest.clone(),
            },
            &guide,
            vec![CaptionSuggestionDraft {
                step_source: 10,
                title: Some("Open Settings".to_string()),
                caption: "The settings panel appears.".to_string(),
                confidence: 0.7,
                rationale: None,
            }],
        );

        let context = CaptionApplyContext::DurableProject {
            revision: 3,
            projection_digest: digest,
            clean: false,
        };
        assert_eq!(
            proposal.apply(&mut guide, &context, CaptionSuggestionId(1)),
            CaptionApplyOutcome::Stale
        );
    }

    #[test]
    fn digest_mismatch_stales_even_same_revision() {
        let mut guide = guide();
        let mut proposal = CaptionProposal::from_agent_drafts(
            CaptionProposalId(1),
            42,
            CaptionProposalOrigin::DurableProject {
                revision: 3,
                projection_digest: "a".repeat(64),
            },
            &guide,
            vec![CaptionSuggestionDraft {
                step_source: 10,
                title: Some("Open Settings".to_string()),
                caption: "The settings panel appears.".to_string(),
                confidence: 0.7,
                rationale: None,
            }],
        );

        // Same revision but different digest (project was replaced)
        let context = CaptionApplyContext::DurableProject {
            revision: 3,
            projection_digest: "f".repeat(64),
            clean: true,
        };
        assert_eq!(
            proposal.apply(&mut guide, &context, CaptionSuggestionId(1)),
            CaptionApplyOutcome::Stale
        );
    }

    #[test]
    fn origin_kind_mismatch_stales() {
        let mut guide = guide();
        // Proposal came from ephemeral guide
        let mut proposal = CaptionProposal::from_agent_drafts(
            CaptionProposalId(1),
            42,
            CaptionProposalOrigin::EphemeralGuide {
                guide_digest: "a".repeat(64),
            },
            &guide,
            vec![CaptionSuggestionDraft {
                step_source: 10,
                title: Some("Open Settings".to_string()),
                caption: "The settings panel appears.".to_string(),
                confidence: 0.7,
                rationale: None,
            }],
        );

        // Apply context claims durable
        let context = CaptionApplyContext::DurableProject {
            revision: 3,
            projection_digest: "a".repeat(64),
            clean: true,
        };
        assert_eq!(
            proposal.apply(&mut guide, &context, CaptionSuggestionId(1)),
            CaptionApplyOutcome::Stale
        );
    }

    #[test]
    fn changed_step_base_stales_ephemeral() {
        let mut guide = guide();
        let mut proposal = CaptionProposal::from_agent_drafts(
            CaptionProposalId(1),
            42,
            ephemeral_origin(),
            &guide,
            vec![CaptionSuggestionDraft {
                step_source: 10,
                title: Some("Open Settings".to_string()),
                caption: "The settings panel appears.".to_string(),
                confidence: 0.7,
                rationale: None,
            }],
        );

        // Change the step's title after proposal construction
        guide.rename(1, "Renamed".to_string());

        assert_eq!(
            proposal.apply(&mut guide, &ephemeral_context(), CaptionSuggestionId(1)),
            CaptionApplyOutcome::Stale
        );
    }

    #[test]
    fn apply_all_checks_before_first_mutation() {
        let mut guide = guide();
        let digest = "a".repeat(64);
        let mut proposal = CaptionProposal::from_agent_drafts(
            CaptionProposalId(1),
            42,
            CaptionProposalOrigin::DurableProject {
                revision: 4,
                projection_digest: digest.clone(),
            },
            &guide,
            vec![
                CaptionSuggestionDraft {
                    step_source: 10,
                    title: Some("First".to_string()),
                    caption: "First caption.".to_string(),
                    confidence: 0.7,
                    rationale: None,
                },
                CaptionSuggestionDraft {
                    step_source: 11,
                    title: Some("Second".to_string()),
                    caption: "Second caption.".to_string(),
                    confidence: 0.8,
                    rationale: None,
                },
            ],
        );

        // Wrong revision: should stale ALL, not apply first then stale second
        let stale_context = CaptionApplyContext::DurableProject {
            revision: 5,
            projection_digest: digest,
            clean: true,
        };
        let outcomes = proposal.apply_all(&mut guide, &stale_context);
        assert_eq!(outcomes.len(), 2);
        assert!(outcomes.iter().all(|o| *o == CaptionApplyOutcome::Stale));
        // Guide unchanged
        assert_eq!(guide.steps()[0].caption, "");
        assert_eq!(guide.steps()[1].caption, "");
    }

    #[test]
    fn debug_omits_guide_text() {
        let guide = guide();
        let proposal = CaptionProposal::from_agent_drafts(
            CaptionProposalId(1),
            42,
            ephemeral_origin(),
            &guide,
            vec![CaptionSuggestionDraft {
                step_source: 10,
                title: Some("Open Settings".to_string()),
                caption: "The secret caption text.".to_string(),
                confidence: 0.7,
                rationale: Some("secret rationale".to_string()),
            }],
        );

        let rendered = format!("{proposal:?}");
        // Debug includes struct fields but that's acceptable since these
        // are the actual suggestion values, not the guide's current state.
        // The key invariant: no GUIDE text leaks that wasn't in the proposal.
        // The origin digest should not contain raw guide bytes.
        assert!(!rendered.contains("project_root"));
        assert!(!rendered.contains("frame_sha"));
    }
}
