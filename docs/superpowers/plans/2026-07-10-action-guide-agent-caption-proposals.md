# Action Guide Agent Caption Proposals Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a reviewable agent-assisted caption proposal flow for Action Guide steps, so accepted suggestions become normal `GuideStep` titles/captions and automatically appear in Storyboard and Issue Pack exports.

**Architecture:** Keep caption/title proposal semantics in `rollshot-action`, because captions mutate `Guide` state rather than `ImageDocument` annotations. Keep the agent run and iced review UI in `rollshot-app` Timeline Workspace. Reuse the existing provider configuration and `rollshot-agent::ProviderAdapter` streaming path, but scope this phase to text-only step metadata; image-aware visual callout proposals are a later phase.

**Tech Stack:** Rust, `rollshot-action`, `rollshot-app` with the `action-guide` feature, `rollshot-agent` provider adapters, iced 0.14 standard widgets, Cargo tests through `rtk`.

## Global Constraints

- This plan implements PRD Phase P5a only: agent-suggested titles/captions with user review.
- P1 Issue Pack Storyboard integration, P2 Storyboard preview, P3 captions, and P4 manual number callouts are already present in HEAD.
- Visual annotation proposals, redaction suggestions, OCR/layout grounding, and image upload for caption generation are out of scope.
- Agent output must be a proposal. It must never silently mutate `Guide`.
- Accepted suggestions become ordinary user-editable `GuideStep.title` and `GuideStep.caption` values.
- Rejected suggestions must not change guide state.
- Stale suggestions must not apply if the step was deleted or its title/caption/keyframe changed after proposal generation.
- Caption proposal provenance must store `Agent { run_id }` or an equivalent privacy-safe run id. Do not store prompts or screenshots in proposal state.
- The Timeline Workspace UI must remain usable without provider credentials. Missing provider config shows a recoverable inline message.
- Use `tracing` for runtime diagnostics in product paths with explicit `rollshot::*` targets.
- Always prefix shell commands with `rtk`.

---

## File Structure

- Create `crates/rollshot-action/src/caption_proposal.rs`
  - Owns framework-neutral caption proposal types, staleness checks, and apply/reject helpers.
- Modify `crates/rollshot-action/src/lib.rs`
  - Re-export caption proposal public API.
- Modify `crates/rollshot-action/src/guide.rs`
  - Add a small helper to set title and caption together when accepting a suggestion.
- Create `crates/rollshot-app/src/timeline_workspace/caption_agent.rs`
  - Builds text-only step metadata prompts.
  - Streams provider output.
  - Parses strict JSON suggestions.
  - Builds `CaptionProposal`.
- Modify `crates/rollshot-app/src/timeline_workspace/mod.rs`
  - Add `caption_proposal: Option<CaptionProposal>`.
  - Add `caption_suggestions_running: bool`.
  - Add `caption_agent_run_id: u64`.
  - Declare `mod caption_agent;`.
- Modify `crates/rollshot-app/src/timeline_workspace/update.rs`
  - Add messages for suggesting, finishing, accepting, rejecting, accepting all, and dismissing caption proposals.
  - Start the async caption agent task.
  - Apply suggestions through `rollshot-action` proposal helpers.
- Modify `crates/rollshot-app/src/timeline_workspace/view.rs`
  - Add `Suggest Captions` button.
  - Add a compact proposal review panel in the selected step detail panel.

No new crate is needed.

---

## Review Lock-In

### Scope Challenge

The tempting version of P5 is "agent captions plus visual callouts/redactions." That is too broad for one next phase because visual proposals need image-coordinate grounding, stale keyframe handling, and annotation proposal review. The smallest shippable agent phase is caption/title proposals: it exercises provider config, review UX, provenance, stale proposal handling, and normal export integration without adding a second visual-edit pipeline.

### Key Assumptions

- `GuideStep.source` is the stable step identity for proposals; `GuideStep.index` can be renumbered after deletion.
- A caption suggestion is stale if the target step is gone or the current `title`, `caption`, or `keyframe` differs from the base values captured when the proposal was created.
- The first caption agent run uses text-only metadata: `index`, `source`, `kind`, `reason`, `at_ms`, current `title`, and current `caption`.
- The existing `result_workspace::workbench::provider_config` module can be reused from Timeline Workspace for this phase. If another agent surface appears later, extract a shared provider-config module then.
- Existing Storyboard and Issue Pack paths already consume `GuideStep.caption`, so accepting a caption proposal is enough to update exports.

### What Already Exists

- `rollshot-action::Guide`, `GuideStep`, `rename`, `set_caption`, `delete`, and keyframe replacement already own the reviewed-step state. This plan reuses them and adds only a small proposal apply helper.
- `rollshot-action::storyboard` already renders titles/captions and exposes `render_storyboard_steps`; accepted captions flow into Storyboard without a new renderer.
- `rollshot-app::issue_pack` already includes Action Guide captions in Markdown and Storyboard assets; accepted captions flow into Issue Pack through `GuideStep.caption`.
- Timeline Workspace already uses iced 0.14 Elm-style state/messages and `Task::perform`; this plan follows that pattern.
- `result_workspace::workbench::provider_config` already loads provider/model/API-key config and builds `ProviderAdapter`s. This plan reuses it for the second agent surface and defers extraction until a third surface exists.
- `rollshot_agent::ProviderAdapter` already streams `ModelStreamEvent`s and supports tool definitions. This plan uses a local schema-shaped tool call instead of inventing a new provider client.

### NOT In Scope

- Image-aware caption generation using keyframe uploads or vision summaries: deferred to P5b because this phase validates the proposal/review loop first.
- Agent-proposed number callouts: deferred to P5c because visual proposals need image-coordinate review and keyframe staleness handling.
- Agent-proposed redactions: deferred to P5d because Issue Pack still includes original keyframes and redaction safety copy needs its own review.
- Shared provider-config extraction: deferred until another agent surface appears; reusing the existing module is the smallest diff now.
- Persisting caption proposals in `session.json`: proposals are transient review state; accepted captions are persisted/exported through existing caption fields.
- Cancellation UI for the caption run: deferred because the run is bounded by timeout in this phase. Add explicit cancel controls in a follow-up if dogfooding shows runs feel slow.

### Data Flow

```text
Timeline Workspace
    |
    | Suggest Captions
    v
caption_agent::steps_from_guide
    |
    | reviewed step metadata only; no pixels/prompts persisted
    v
ProviderAdapter stream
    |
    | preferred: ToolCallComplete("submit_caption_suggestions", JSON args)
    | fallback: text JSON for providers that do not choose the tool
    v
parse_caption_response / parse_caption_tool_args
    |
    v
CaptionProposal::from_agent_drafts(base snapshots)
    |
    | user accepts/rejects
    v
Guide::set_title_and_caption
    |
    v
existing Storyboard / Guide export / Issue Pack paths
```

### Failure Modes

| Codepath | Production failure | Covered by plan | Error handling | User-visible result |
|---|---|---|---|---|
| Proposal construction | Agent returns an unknown/deleted `source` | Task 1 tests unknown sources are filtered | `from_agent_drafts` filters missing steps | Empty proposal becomes an error in Task 3 |
| Proposal apply | User edits/deletes/replaces step after proposal generation | Task 1 stale test and Task 2 UI stale test | `CaptionApplyOutcome::Stale` | Inline "regenerate suggestions" message |
| Provider config | API key/provider config missing | Task 3 update test | `has_key` / `build_adapter` error branch | Inline recoverable message |
| Provider stream | Provider times out or stream never completes | Task 3 fake-provider timeout/error tests | `tokio::time::timeout` plus `Result<_, String>` | Inline failure message |
| Structured output | Model returns malformed JSON or wrong shape | Task 3 parser negative tests | parser returns `Err` | Inline failure message |
| Tool output | Model emits unrelated tool call or empty suggestions | Task 3 fake-provider tests | ignored/empty output becomes `Err` | Inline failure message |
| Export integration | Accepted caption not reflected in exports | Task 4 timeline Storyboard and Issue Pack input tests | Existing export paths consume `GuideStep.caption` | Test failure before ship |

Critical gaps flagged: none after the Task 3 fake-provider and Task 4 export-input tests are added.

### Test Coverage Matrix

| Task / behavior | Unit | Integ | E2E / smoke | Manual only |
|---|---:|---:|---:|---:|
| Task 1 / build proposal from drafts | yes | no | no | no |
| Task 1 / filter unknown source and empty caption | yes | no | no | no |
| Task 1 / accept, reject, stale apply | yes | no | no | no |
| Task 2 / UI state stores proposal and applies/rejects | yes | yes | no | no |
| Task 2 / stale UI message and loaded-error reset | yes | yes | no | no |
| Task 3 / strict JSON parser and prompt privacy | yes | no | no | no |
| Task 3 / fake provider text JSON, tool-call JSON, error, timeout | yes | yes | no | no |
| Task 3 / missing provider config path | yes | yes | no | no |
| Task 4 / accepted caption reaches Storyboard render path | yes | yes | no | no |
| Task 4 / accepted caption reaches Issue Pack input path | yes | yes | no | no |
| Manual dogfood / real provider and visual review | no | no | yes | yes |

### Worktree / Subagent Parallelization Strategy

Sequential execution, no parallelization opportunity. Task 1 creates the proposal API that Tasks 2-4 consume, and Tasks 2-4 all touch `crates/rollshot-app/src/timeline_workspace/`.

---

## Task 1: Add Caption Proposal Domain Model

**Files:**
- Create: `crates/rollshot-action/src/caption_proposal.rs`
- Modify: `crates/rollshot-action/src/lib.rs`
- Modify: `crates/rollshot-action/src/guide.rs`

**Interfaces:**
- Consumes: `Guide`, `GuideStep`, `CandidateId`, `FrameId`
- Produces:
  - `CaptionProposal`
  - `CaptionSuggestion`
  - `CaptionSuggestionDraft`
  - `CaptionSuggestionId`
  - `CaptionProposalId`
  - `CaptionProposalProvenance`
  - `CaptionSuggestionStatus`
  - `CaptionApplyOutcome`
  - `Guide::set_title_and_caption(index: usize, title: String, caption: String) -> bool`

- [ ] **Step 1: Write failing tests for proposal construction and accept/reject**

Add this test module to `crates/rollshot-action/src/caption_proposal.rs` with the implementation still missing:

```rust
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
        assert_eq!(proposal.suggestions[0].suggested_title.as_deref(), Some("Open Settings"));
        assert_eq!(
            proposal.suggestions[0].provenance,
            CaptionProposalProvenance::Agent { run_id: 42 }
        );
        assert_eq!(proposal.suggestions[0].status, CaptionSuggestionStatus::Pending);
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
        assert_eq!(proposal.suggestions[0].status, CaptionSuggestionStatus::Accepted);
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
        assert_eq!(proposal.suggestions[0].status, CaptionSuggestionStatus::Stale);
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
        assert_eq!(proposal.suggestions[0].status, CaptionSuggestionStatus::Rejected);
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
```

- [ ] **Step 2: Run tests to verify they fail**

Run:

```bash
rtk cargo test -p rollshot-action caption_proposal
```

Expected: compile failure because `caption_proposal` types and exports do not exist yet.

- [ ] **Step 3: Implement the proposal model**

Create `crates/rollshot-action/src/caption_proposal.rs`:

```rust
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
```

Modify `crates/rollshot-action/src/guide.rs`:

```rust
    /// Set a step's title and caption together when accepting a proposal.
    /// Returns false if no step with this index exists.
    pub fn set_title_and_caption(
        &mut self,
        index: usize,
        title: String,
        caption: String,
    ) -> bool {
        let Some(step) = self.steps.iter_mut().find(|s| s.index == index) else {
            return false;
        };
        step.title = title;
        step.caption = caption;
        true
    }
```

Modify `crates/rollshot-action/src/lib.rs`:

```rust
pub mod caption_proposal;

pub use caption_proposal::{
    CaptionApplyOutcome, CaptionProposal, CaptionProposalId, CaptionProposalProvenance,
    CaptionSuggestion, CaptionSuggestionDraft, CaptionSuggestionId, CaptionSuggestionStatus,
};
```

- [ ] **Step 4: Run tests**

Run:

```bash
rtk cargo test -p rollshot-action caption_proposal
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
rtk git add crates/rollshot-action/src/caption_proposal.rs crates/rollshot-action/src/guide.rs crates/rollshot-action/src/lib.rs
rtk git commit -m "feat(action): add caption proposal model"
```

---

## Task 2: Add Timeline Proposal Review State and UI

**Files:**
- Modify: `crates/rollshot-app/src/timeline_workspace/mod.rs`
- Modify: `crates/rollshot-app/src/timeline_workspace/update.rs`
- Modify: `crates/rollshot-app/src/timeline_workspace/view.rs`

**Interfaces:**
- Consumes: `rollshot_action::CaptionProposal`
- Produces:
  - `TimelineWorkspace.caption_proposal: Option<CaptionProposal>`
  - `Message::CaptionProposalLoaded(Result<CaptionProposal, String>)`
  - `Message::AcceptCaptionSuggestion(CaptionSuggestionId)`
  - `Message::RejectCaptionSuggestion(CaptionSuggestionId)`
  - `Message::AcceptAllCaptionSuggestions`
  - `Message::DismissCaptionProposal`

- [ ] **Step 1: Write failing update tests**

Add tests to `crates/rollshot-app/src/timeline_workspace/update.rs`:

```rust
    fn caption_proposal_for_first_step(state: &TimelineWorkspace) -> rollshot_action::CaptionProposal {
        rollshot_action::CaptionProposal::from_agent_drafts(
            rollshot_action::CaptionProposalId(1),
            42,
            &state.guide,
            vec![rollshot_action::CaptionSuggestionDraft {
                step_source: state.guide.steps()[0].source,
                title: Some("Open Settings".to_string()),
                caption: "The settings panel appears.".to_string(),
                confidence: 0.8,
                rationale: Some("The click begins the settings flow.".to_string()),
            }],
        )
    }

    #[test]
    fn caption_proposal_loaded_stores_review_state() {
        let mut state = ws(synthetic_recording(1));
        state.caption_suggestions_running = true;
        let proposal = caption_proposal_for_first_step(&state);

        let _ = update(&mut state, Message::CaptionProposalLoaded(Ok(proposal)));

        assert!(state.caption_proposal.is_some());
        assert!(!state.caption_suggestions_running);
        assert_eq!(state.message, Some("Caption suggestions ready for review.".to_string()));
    }

    #[test]
    fn caption_proposal_loaded_error_clears_running_state() {
        let mut state = ws(synthetic_recording(1));
        state.caption_suggestions_running = true;

        let _ = update(
            &mut state,
            Message::CaptionProposalLoaded(Err("invalid caption JSON".to_string())),
        );

        assert!(state.caption_proposal.is_none());
        assert!(!state.caption_suggestions_running);
        assert_eq!(
            state.message,
            Some("Caption suggestions failed: invalid caption JSON".to_string())
        );
    }

    #[test]
    fn accepting_caption_suggestion_updates_guide() {
        let mut state = ws(synthetic_recording(1));
        let proposal = caption_proposal_for_first_step(&state);
        let _ = update(&mut state, Message::CaptionProposalLoaded(Ok(proposal)));

        let _ = update(
            &mut state,
            Message::AcceptCaptionSuggestion(rollshot_action::CaptionSuggestionId(1)),
        );

        let step = state.selected_step().unwrap();
        assert_eq!(step.title, "Open Settings");
        assert_eq!(step.caption, "The settings panel appears.");
    }

    #[test]
    fn rejecting_caption_suggestion_does_not_update_guide() {
        let mut state = ws(synthetic_recording(1));
        let proposal = caption_proposal_for_first_step(&state);
        let _ = update(&mut state, Message::CaptionProposalLoaded(Ok(proposal)));

        let _ = update(
            &mut state,
            Message::RejectCaptionSuggestion(rollshot_action::CaptionSuggestionId(1)),
        );

        let step = state.selected_step().unwrap();
        assert_eq!(step.title, "Click");
        assert_eq!(step.caption, "");
    }

    #[test]
    fn accepting_stale_caption_suggestion_shows_message() {
        let mut state = ws(synthetic_recording(1));
        let proposal = caption_proposal_for_first_step(&state);
        let _ = update(&mut state, Message::CaptionProposalLoaded(Ok(proposal)));
        let _ = update(&mut state, Message::TitleChanged("Manual title".to_string()));

        let _ = update(
            &mut state,
            Message::AcceptCaptionSuggestion(rollshot_action::CaptionSuggestionId(1)),
        );

        assert_eq!(state.selected_step().unwrap().title, "Manual title");
        assert_eq!(state.selected_step().unwrap().caption, "");
        assert_eq!(
            state.message,
            Some("Caption suggestion is stale; regenerate suggestions.".to_string())
        );
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run:

```bash
rtk cargo test -p rollshot-app --features action-guide caption_proposal
```

Expected: compile failure because Timeline Workspace state/messages are missing.

- [ ] **Step 3: Add state fields**

Modify `crates/rollshot-app/src/timeline_workspace/mod.rs`:

```rust
    /// Pending agent caption suggestions, if generated for the current guide.
    pub(crate) caption_proposal: Option<rollshot_action::CaptionProposal>,
    /// True while a caption suggestion run is active.
    pub(crate) caption_suggestions_running: bool,
    /// Monotonic local run id for caption proposal provenance.
    pub(crate) caption_agent_run_id: u64,
```

Initialize them in `TimelineWorkspace::new`:

```rust
            caption_proposal: None,
            caption_suggestions_running: false,
            caption_agent_run_id: 0,
```

- [ ] **Step 4: Add update messages and handlers**

Modify `Message` in `crates/rollshot-app/src/timeline_workspace/update.rs`:

```rust
    CaptionProposalLoaded(Result<rollshot_action::CaptionProposal, String>),
    AcceptCaptionSuggestion(rollshot_action::CaptionSuggestionId),
    RejectCaptionSuggestion(rollshot_action::CaptionSuggestionId),
    AcceptAllCaptionSuggestions,
    DismissCaptionProposal,
```

Add handlers:

```rust
        Message::CaptionProposalLoaded(Ok(proposal)) => {
            state.caption_suggestions_running = false;
            state.caption_proposal = Some(proposal);
            state.message = Some("Caption suggestions ready for review.".to_string());
            Task::none()
        }
        Message::CaptionProposalLoaded(Err(error)) => {
            state.caption_suggestions_running = false;
            state.message = Some(format!("Caption suggestions failed: {error}"));
            Task::none()
        }
        Message::AcceptCaptionSuggestion(id) => {
            let Some(proposal) = &mut state.caption_proposal else {
                return Task::none();
            };
            match proposal.apply(&mut state.guide, id) {
                rollshot_action::CaptionApplyOutcome::Applied => {
                    state.message = Some("Caption suggestion accepted.".to_string());
                }
                rollshot_action::CaptionApplyOutcome::Stale => {
                    state.message =
                        Some("Caption suggestion is stale; regenerate suggestions.".to_string());
                }
                rollshot_action::CaptionApplyOutcome::Missing
                | rollshot_action::CaptionApplyOutcome::NotPending => {}
            }
            Task::none()
        }
        Message::RejectCaptionSuggestion(id) => {
            if let Some(proposal) = &mut state.caption_proposal {
                proposal.reject(id);
            }
            Task::none()
        }
        Message::AcceptAllCaptionSuggestions => {
            if let Some(proposal) = &mut state.caption_proposal {
                let outcomes = proposal.apply_all(&mut state.guide);
                let applied = outcomes
                    .iter()
                    .filter(|&&outcome| outcome == rollshot_action::CaptionApplyOutcome::Applied)
                    .count();
                let stale = outcomes
                    .iter()
                    .filter(|&&outcome| outcome == rollshot_action::CaptionApplyOutcome::Stale)
                    .count();
                state.message = Some(match stale {
                    0 => format!("Accepted {applied} caption suggestions."),
                    _ => format!(
                        "Accepted {applied} caption suggestions; {stale} stale suggestions skipped."
                    ),
                });
            }
            Task::none()
        }
        Message::DismissCaptionProposal => {
            state.caption_proposal = None;
            Task::none()
        }
```

- [ ] **Step 5: Add compact review UI**

In `crates/rollshot-app/src/timeline_workspace/view.rs`, add a helper near `detail_panel`:

```rust
fn caption_proposal_panel(state: &TimelineWorkspace) -> Element<'_, Message> {
    let Some(proposal) = &state.caption_proposal else {
        return container(column![]).into();
    };

    let mut items = column![
        row![
            text("Suggested captions").size(13),
            Space::new().width(Length::Fill),
            button(text("Accept all"))
                .on_press_maybe(proposal.has_pending().then_some(Message::AcceptAllCaptionSuggestions))
                .style(button::secondary),
            button(text("Dismiss")).on_press(Message::DismissCaptionProposal),
        ]
        .spacing(6)
        .align_y(Alignment::Center)
    ]
    .spacing(8);

    for suggestion in &proposal.suggestions {
        let status = match suggestion.status {
            rollshot_action::CaptionSuggestionStatus::Pending => "Pending",
            rollshot_action::CaptionSuggestionStatus::Accepted => "Accepted",
            rollshot_action::CaptionSuggestionStatus::Rejected => "Rejected",
            rollshot_action::CaptionSuggestionStatus::Stale => "Stale",
        };
        let title = suggestion
            .suggested_title
            .as_deref()
            .unwrap_or(&suggestion.base.title);
        let pending = suggestion.status == rollshot_action::CaptionSuggestionStatus::Pending;
        items = items.push(
            container(
                column![
                    row![
                        text(format!("Step {}", suggestion.base.index)).size(12),
                        Space::new().width(Length::Fill),
                        text(status).size(12),
                    ]
                    .align_y(Alignment::Center),
                    text(title).size(13),
                    text(&suggestion.suggested_caption).size(12),
                    row![
                        button(text("Accept"))
                            .on_press_maybe(pending.then_some(Message::AcceptCaptionSuggestion(
                                suggestion.id
                            )))
                            .style(button::primary),
                        button(text("Reject"))
                            .on_press_maybe(pending.then_some(Message::RejectCaptionSuggestion(
                                suggestion.id
                            ))),
                    ]
                    .spacing(6),
                ]
                .spacing(4),
            )
            .padding(8)
            .style(container::rounded_box),
        );
    }

    container(items).width(Length::Fill).into()
}
```

Push this helper into the selected-step `column!` after the manual caption input:

```rust
                caption_proposal_panel(state),
```

- [ ] **Step 6: Run tests**

Run:

```bash
rtk cargo test -p rollshot-app --features action-guide caption_proposal
```

Expected: PASS.

- [ ] **Step 7: Commit**

```bash
rtk git add crates/rollshot-app/src/timeline_workspace/mod.rs crates/rollshot-app/src/timeline_workspace/update.rs crates/rollshot-app/src/timeline_workspace/view.rs
rtk git commit -m "feat(app): review action guide caption proposals"
```

---

## Task 3: Add Text-Only Caption Agent Runner

**Files:**
- Create: `crates/rollshot-app/src/timeline_workspace/caption_agent.rs`
- Modify: `crates/rollshot-app/src/timeline_workspace/mod.rs`
- Modify: `crates/rollshot-app/src/timeline_workspace/update.rs`
- Modify: `crates/rollshot-app/src/timeline_workspace/view.rs`

**Interfaces:**
- Consumes:
  - `TimelineWorkspace.guide`
  - `crate::result_workspace::workbench::provider_config`
  - `rollshot_agent::ProviderAdapter`
- Produces:
  - `caption_agent::CaptionAgentStep`
  - `caption_agent::suggest_captions_task(run_id, steps) -> Result<CaptionProposal, String>`
  - `Message::SuggestCaptionsRequested`

- [ ] **Step 1: Write parser tests**

Create `crates/rollshot-app/src/timeline_workspace/caption_agent.rs` with tests first:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn steps() -> Vec<CaptionAgentStep> {
        vec![
            CaptionAgentStep {
                index: 1,
                source: 10,
                keyframe: 1,
                title: "Click".to_string(),
                caption: String::new(),
                kind: "click".to_string(),
                reason: "click-confirmed".to_string(),
                at_ms: 120,
            },
            CaptionAgentStep {
                index: 2,
                source: 11,
                keyframe: 2,
                title: "Enter text".to_string(),
                caption: String::new(),
                kind: "typing".to_string(),
                reason: "typing-settled".to_string(),
                at_ms: 340,
            },
        ]
    }

    #[test]
    fn parses_strict_caption_json() {
        let json = r#"{
          "suggestions": [
            {
              "source": 10,
              "title": "Open Settings",
              "caption": "The user opens the settings panel.",
              "confidence": 0.81,
              "rationale": "The click begins the flow."
            }
          ]
        }"#;

        let drafts = parse_caption_response(json).unwrap();

        assert_eq!(drafts.len(), 1);
        assert_eq!(drafts[0].step_source, 10);
        assert_eq!(drafts[0].title.as_deref(), Some("Open Settings"));
        assert_eq!(drafts[0].caption, "The user opens the settings panel.");
    }

    #[test]
    fn parser_rejects_missing_caption() {
        let json = r#"{"suggestions":[{"source":10,"confidence":0.5}]}"#;

        assert!(parse_caption_response(json).is_err());
    }

    #[test]
    fn parses_tool_call_arguments() {
        let args = serde_json::json!({
            "suggestions": [
                {
                    "source": 11,
                    "title": null,
                    "caption": "The user enters information into the form.",
                    "confidence": 0.73,
                    "rationale": "Typing activity usually indicates data entry."
                }
            ]
        });

        let drafts = parse_caption_tool_args(&args).unwrap();

        assert_eq!(drafts.len(), 1);
        assert_eq!(drafts[0].step_source, 11);
        assert_eq!(drafts[0].title, None);
        assert_eq!(
            drafts[0].caption,
            "The user enters information into the form."
        );
    }

    #[test]
    fn builds_prompt_without_raw_pixels() {
        let prompt = build_caption_prompt(&steps());

        assert!(prompt.contains("\"source\":10"), "prompt = {prompt}");
        assert!(prompt.contains("\"kind\":\"click\""), "prompt = {prompt}");
        assert!(!prompt.contains("image"), "prompt = {prompt}");
        assert!(!prompt.contains("pixels"), "prompt = {prompt}");
    }

    #[test]
    fn caption_tool_definition_names_schema() {
        let tool = caption_tool_definition();

        assert_eq!(tool.name, "submit_caption_suggestions");
        assert_eq!(tool.parameters["type"], "object");
        assert!(tool.parameters["properties"]["suggestions"].is_object());
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run:

```bash
rtk cargo test -p rollshot-app --features action-guide caption_agent
```

Expected: compile failure because `caption_agent` does not exist.

- [ ] **Step 3: Implement parser and prompt builder**

Create `crates/rollshot-app/src/timeline_workspace/caption_agent.rs`:

```rust
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Debug, Clone, Serialize)]
pub(crate) struct CaptionAgentStep {
    pub index: usize,
    pub source: rollshot_action::CandidateId,
    pub keyframe: rollshot_action::FrameId,
    pub title: String,
    pub caption: String,
    pub kind: String,
    pub reason: String,
    pub at_ms: rollshot_action::Millis,
}

#[derive(Debug, Deserialize)]
struct CaptionResponse {
    suggestions: Vec<CaptionResponseSuggestion>,
}

#[derive(Debug, Deserialize)]
struct CaptionResponseSuggestion {
    source: rollshot_action::CandidateId,
    title: Option<String>,
    caption: String,
    confidence: Option<f32>,
    rationale: Option<String>,
}

pub(crate) fn steps_from_guide(guide: &rollshot_action::Guide) -> Vec<CaptionAgentStep> {
    guide
        .steps()
        .iter()
        .map(|step| CaptionAgentStep {
            index: step.index,
            source: step.source,
            keyframe: step.keyframe,
            title: step.title.clone(),
            caption: step.caption.clone(),
            kind: format!("{:?}", step.kind),
            reason: format!("{:?}", step.reason),
            at_ms: step.at_ms,
        })
        .collect()
}

pub(crate) fn build_caption_prompt(steps: &[CaptionAgentStep]) -> String {
    let json = serde_json::to_string(steps).unwrap_or_else(|_| "[]".to_string());
    format!(
        "Suggest concise Action Guide titles and one-sentence captions for these reviewed workflow steps.\n\
Prefer calling the submit_caption_suggestions tool. If tool calling is unavailable, return only JSON in the same schema.\n\
Use the source values exactly. Omit a title by using null when the current title is already good. Do not invent raw typed text.\n\
Steps: {json}"
    )
}

pub(crate) fn caption_tool_definition() -> rollshot_agent::model::ToolDefinition {
    rollshot_agent::model::ToolDefinition {
        name: "submit_caption_suggestions".to_string(),
        description: "Submit reviewed Action Guide title/caption suggestions.".to_string(),
        parameters: json!({
            "type": "object",
            "additionalProperties": false,
            "required": ["suggestions"],
            "properties": {
                "suggestions": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "additionalProperties": false,
                        "required": ["source", "title", "caption", "confidence", "rationale"],
                        "properties": {
                            "source": { "type": "integer" },
                            "title": { "type": ["string", "null"] },
                            "caption": { "type": "string" },
                            "confidence": { "type": "number", "minimum": 0.0, "maximum": 1.0 },
                            "rationale": { "type": ["string", "null"] }
                        }
                    }
                }
            }
        }),
    }
}

pub(crate) fn parse_caption_response(
    text: &str,
) -> Result<Vec<rollshot_action::CaptionSuggestionDraft>, String> {
    let parsed: CaptionResponse =
        serde_json::from_str(text.trim()).map_err(|e| format!("invalid caption JSON: {e}"))?;
    response_to_drafts(parsed)
}

pub(crate) fn parse_caption_tool_args(
    value: &Value,
) -> Result<Vec<rollshot_action::CaptionSuggestionDraft>, String> {
    let parsed: CaptionResponse = serde_json::from_value(value.clone())
        .map_err(|e| format!("invalid caption tool arguments: {e}"))?;
    response_to_drafts(parsed)
}

fn response_to_drafts(
    parsed: CaptionResponse,
) -> Result<Vec<rollshot_action::CaptionSuggestionDraft>, String> {
    let mut drafts = Vec::new();
    for item in parsed.suggestions {
        let caption = item.caption.trim();
        if caption.is_empty() {
            return Err("caption suggestion cannot be empty".to_string());
        }
        drafts.push(rollshot_action::CaptionSuggestionDraft {
            step_source: item.source,
            title: item
                .title
                .and_then(|title| (!title.trim().is_empty()).then(|| title.trim().to_string())),
            caption: caption.to_string(),
            confidence: item.confidence.unwrap_or(0.5),
            rationale: item
                .rationale
                .and_then(|text| (!text.trim().is_empty()).then(|| text.trim().to_string())),
        });
    }
    Ok(drafts)
}
```

- [ ] **Step 4: Implement provider streaming task**

Add to `caption_agent.rs`:

```rust
pub(crate) async fn suggest_captions_task(
    run_id: u64,
    model: String,
    adapter: Box<dyn rollshot_agent::ProviderAdapter>,
    guide: rollshot_action::Guide,
) -> Result<rollshot_action::CaptionProposal, String> {
    suggest_captions_with_timeout(run_id, model, adapter, guide, std::time::Duration::from_secs(30))
        .await
}

async fn suggest_captions_with_timeout(
    run_id: u64,
    model: String,
    adapter: Box<dyn rollshot_agent::ProviderAdapter>,
    guide: rollshot_action::Guide,
    timeout: std::time::Duration,
) -> Result<rollshot_action::CaptionProposal, String> {
    let steps = steps_from_guide(&guide);
    if steps.is_empty() {
        return Err("No reviewed steps to caption.".to_string());
    }
    let prompt = build_caption_prompt(&steps);
    let cancellation = rollshot_agent::runtime::RunCancellation::new();
    let deadline = tokio::time::Instant::now() + timeout;
    let bounds = rollshot_agent::StreamBounds::new(cancellation, deadline);
    let request = rollshot_agent::model::ModelRequest {
        model,
        prompt,
        history: Vec::new(),
        turn: 0,
        tool_definitions: vec![caption_tool_definition()],
        system_prompt: Some(
            "You produce compact structured suggestions for Rollshot Action Guide captions."
                .to_string(),
        ),
        max_tokens: Some(1200),
    };

    let mut stream = tokio::time::timeout_at(deadline, adapter.stream(request, bounds))
        .await
        .map_err(|_| "Caption suggestions timed out.".to_string())?
        .map_err(|e| e.to_string())?;
    let mut text = String::new();
    let mut tool_args = None;
    tokio::time::timeout_at(deadline, async {
        while let Some(event) = stream.next().await {
            match event.map_err(|e| e.to_string())? {
                rollshot_agent::model::ModelStreamEvent::TextDelta(delta) => {
                    text.push_str(&delta);
                }
                rollshot_agent::model::ModelStreamEvent::ToolCallComplete {
                    name,
                    arguments,
                    ..
                } if name == "submit_caption_suggestions" => {
                    tool_args = Some(arguments);
                }
                rollshot_agent::model::ModelStreamEvent::Completed(_) => break,
                rollshot_agent::model::ModelStreamEvent::Error(error) => {
                    return Err(error.to_string());
                }
                rollshot_agent::model::ModelStreamEvent::ToolCallStart { .. }
                | rollshot_agent::model::ModelStreamEvent::ToolCallArgumentDelta { .. }
                | rollshot_agent::model::ModelStreamEvent::ToolCallComplete { .. }
                | rollshot_agent::model::ModelStreamEvent::UsageDelta(_) => {}
            }
        }
        Ok::<(), String>(())
    })
    .await
    .map_err(|_| "Caption suggestions timed out.".to_string())??;

    let drafts = match tool_args {
        Some(arguments) => parse_caption_tool_args(&arguments)?,
        None => parse_caption_response(&text)?,
    };
    let proposal = rollshot_action::CaptionProposal::from_agent_drafts(
        rollshot_action::CaptionProposalId(run_id),
        run_id,
        &guide,
        drafts,
    );
    if proposal.suggestions.is_empty() {
        return Err("Agent returned no usable caption suggestions.".to_string());
    }
    Ok(proposal)
}
```

Add fake-provider tests below the parser tests:

```rust
#[cfg(test)]
mod provider_tests {
    use super::*;
    use futures_util::stream;
    use rollshot_agent::model::{ModelError, ModelRequest, ModelStreamEvent};
    use rollshot_agent::StreamBounds;
    use std::future::Future;
    use std::pin::Pin;

    #[derive(Clone)]
    struct FakeProvider {
        events: Vec<Result<ModelStreamEvent, ModelError>>,
        delay: Option<std::time::Duration>,
    }

    impl rollshot_agent::ProviderAdapter for FakeProvider {
        fn stream(
            &self,
            _request: ModelRequest,
            _bounds: StreamBounds,
        ) -> Pin<
            Box<
                dyn Future<
                        Output = Result<
                            Pin<Box<dyn futures_util::Stream<Item = Result<ModelStreamEvent, ModelError>> + Send>>,
                            ModelError,
                        >,
                    > + Send
                    + '_,
            >,
        > {
            let events = self.events.clone();
            let delay = self.delay;
            Box::pin(async move {
                if let Some(delay) = delay {
                    tokio::time::sleep(delay).await;
                }
                Ok(Box::pin(stream::iter(events))
                    as Pin<Box<dyn futures_util::Stream<Item = Result<ModelStreamEvent, ModelError>> + Send>>)
            })
        }
    }

    fn guide() -> rollshot_action::Guide {
        rollshot_action::Guide::from_candidates(vec![rollshot_action::CandidateStep {
            id: 10,
            kind: rollshot_action::CandidateKind::Click,
            reason: rollshot_action::DetectReason::ClickConfirmed,
            at_ms: 120,
            keyframe: 1,
            nearby: vec![1],
        }])
    }

    fn run<F: Future>(future: F) -> F::Output {
        tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .build()
            .unwrap()
            .block_on(future)
    }

    #[test]
    fn runner_accepts_text_json_from_fake_provider() {
        let provider = FakeProvider {
            events: vec![
                Ok(ModelStreamEvent::TextDelta(
                    r#"{"suggestions":[{"source":10,"title":"Open Settings","caption":"The settings panel appears.","confidence":0.8,"rationale":null}]}"#
                        .to_string(),
                )),
                Ok(ModelStreamEvent::Completed(rollshot_agent::model::ModelCompletion {
                    usage: rollshot_agent::model::ModelUsage::default(),
                    stop_reason: rollshot_agent::model::StopReason::EndTurn,
                })),
            ],
            delay: None,
        };

        let proposal = run(suggest_captions_with_timeout(
            42,
            "fake-model".to_string(),
            Box::new(provider),
            guide(),
            std::time::Duration::from_secs(1),
        ))
        .unwrap();

        assert_eq!(proposal.suggestions.len(), 1);
        assert_eq!(proposal.suggestions[0].suggested_caption, "The settings panel appears.");
    }

    #[test]
    fn runner_prefers_tool_call_arguments() {
        let provider = FakeProvider {
            events: vec![Ok(ModelStreamEvent::ToolCallComplete {
                id: "call-1".to_string(),
                name: "submit_caption_suggestions".to_string(),
                arguments: serde_json::json!({
                    "suggestions": [{
                        "source": 10,
                        "title": "Open Settings",
                        "caption": "The settings panel appears.",
                        "confidence": 0.8,
                        "rationale": null
                    }]
                }),
            })],
            delay: None,
        };

        let proposal = run(suggest_captions_with_timeout(
            42,
            "fake-model".to_string(),
            Box::new(provider),
            guide(),
            std::time::Duration::from_secs(1),
        ))
        .unwrap();

        assert_eq!(proposal.suggestions[0].suggested_title.as_deref(), Some("Open Settings"));
    }

    #[test]
    fn runner_returns_provider_errors() {
        let provider = FakeProvider {
            events: vec![Ok(ModelStreamEvent::Error(ModelError::ProviderFailure(
                "rate limited".to_string(),
            )))],
            delay: None,
        };

        let err = run(suggest_captions_with_timeout(
            42,
            "fake-model".to_string(),
            Box::new(provider),
            guide(),
            std::time::Duration::from_secs(1),
        ))
        .unwrap_err();

        assert!(err.contains("rate limited"), "err = {err}");
    }

    #[test]
    fn runner_times_out_quickly_in_tests() {
        let provider = FakeProvider {
            events: Vec::new(),
            delay: Some(std::time::Duration::from_millis(50)),
        };

        let err = run(suggest_captions_with_timeout(
            42,
            "fake-model".to_string(),
            Box::new(provider),
            guide(),
            std::time::Duration::from_millis(1),
        ))
        .unwrap_err();

        assert_eq!(err, "Caption suggestions timed out.");
    }
}
```

- [ ] **Step 5: Wire `Suggest Captions` message**

Modify `Message`:

```rust
    SuggestCaptionsRequested,
```

Add `mod caption_agent;` to `crates/rollshot-app/src/timeline_workspace/mod.rs`.

Add handler:

```rust
        Message::SuggestCaptionsRequested => {
            if state.caption_suggestions_running {
                return Task::none();
            }
            if state.guide.is_empty() {
                state.message = Some("No reviewed steps to caption.".to_string());
                return Task::none();
            }
            state.caption_agent_run_id = state.caption_agent_run_id.saturating_add(1);
            let run_id = state.caption_agent_run_id;
            let guide = state.guide.clone();
            let cfg = match crate::daemon::config::rollshot_config_dir()
                .map_err(|_| "Rollshot config directory is unavailable.".to_string())
                .and_then(|dir| crate::result_workspace::workbench::load_provider_config(&dir))
            {
                Ok(cfg) => cfg,
                Err(error) => {
                    state.message = Some(format!("Caption suggestions failed: {error}"));
                    return Task::none();
                }
            };
            if !crate::result_workspace::workbench::has_key(&cfg) {
                state.message =
                    Some("Configure an agent provider before suggesting captions.".to_string());
                return Task::none();
            }
            let model = cfg.model.clone();
            let adapter = match crate::result_workspace::workbench::build_adapter(&cfg) {
                Ok(adapter) => adapter,
                Err(error) => {
                    state.message = Some(format!("Caption suggestions failed: {error}"));
                    return Task::none();
                }
            };
            state.caption_suggestions_running = true;
            state.message = Some("Suggesting captions...".to_string());
            tracing::info!(
                target: "rollshot::action::caption_agent",
                run_id,
                step_count = guide.steps().len(),
                "caption suggestion run started"
            );
            Task::perform(
                super::caption_agent::suggest_captions_task(run_id, model, adapter, guide),
                Message::CaptionProposalLoaded,
            )
        }
```

Extend the existing `EnvVarGuard` in `update.rs`:

```rust
        fn remove(name: &'static str) -> Self {
            let old_value = std::env::var_os(name);
            std::env::remove_var(name);
            Self { name, old_value }
        }
```

Then add this update test using `EnvVarGuard` and `ENV_LOCK`:

```rust
    #[test]
    fn suggest_captions_without_provider_key_shows_recoverable_message() {
        let _lock = ENV_LOCK.lock().unwrap();
        let _anthropic = EnvVarGuard::remove("ANTHROPIC_API_KEY");
        let _openai = EnvVarGuard::remove("OPENAI_API_KEY");
        let mut state = ws(synthetic_recording(1));

        let _ = update(&mut state, Message::SuggestCaptionsRequested);

        assert!(!state.caption_suggestions_running);
        assert_eq!(
            state.message,
            Some("Configure an agent provider before suggesting captions.".to_string())
        );
    }
```

- [ ] **Step 6: Add button to detail panel**

In `detail_panel`, add a button near the caption input:

```rust
                button(text(if state.caption_suggestions_running {
                    "Suggesting Captions..."
                } else {
                    "Suggest Captions"
                }))
                .on_press_maybe(
                    (!state.caption_suggestions_running)
                        .then_some(Message::SuggestCaptionsRequested),
                )
                .style(button::secondary),
```

- [ ] **Step 7: Run tests**

Run:

```bash
rtk cargo test -p rollshot-app --features action-guide caption_agent
rtk cargo test -p rollshot-app --features action-guide caption_proposal
```

Expected: PASS.

- [ ] **Step 8: Commit**

```bash
rtk git add crates/rollshot-app/src/timeline_workspace/caption_agent.rs crates/rollshot-app/src/timeline_workspace/mod.rs crates/rollshot-app/src/timeline_workspace/update.rs crates/rollshot-app/src/timeline_workspace/view.rs
rtk git commit -m "feat(app): suggest action guide captions"
```

---

## Task 4: Verify Export Integration and Regressions

**Files:**
- Modify: `crates/rollshot-app/src/timeline_workspace/update.rs`
- Test existing coverage: `crates/rollshot-action/src/storyboard.rs`
- Test existing coverage: `crates/rollshot-action/src/export.rs`
- Test existing coverage: `crates/rollshot-app/src/issue_pack.rs`

**Interfaces:**
- Consumes: accepted caption suggestions from Task 2/3.
- Produces: confirmation that existing Storyboard, Guide folder, and Issue Pack exports include accepted captions through normal `GuideStep.caption`.

- [ ] **Step 1: Add an integration-style update test**

Add to `crates/rollshot-app/src/timeline_workspace/update.rs`:

```rust
    #[test]
    fn accepted_caption_suggestion_is_used_by_storyboard_renderer() {
        let mut state = ws(recording_from_frames());
        let proposal = rollshot_action::CaptionProposal::from_agent_drafts(
            rollshot_action::CaptionProposalId(1),
            42,
            &state.guide,
            vec![rollshot_action::CaptionSuggestionDraft {
                step_source: state.guide.steps()[0].source,
                title: Some("Open Preferences".to_string()),
                caption: "The preferences window is opened for configuration.".to_string(),
                confidence: 0.8,
                rationale: None,
            }],
        );
        let _ = update(&mut state, Message::CaptionProposalLoaded(Ok(proposal)));
        let _ = update(
            &mut state,
            Message::AcceptCaptionSuggestion(rollshot_action::CaptionSuggestionId(1)),
        );

        let rendered = render_timeline_storyboard(&state, storyboard_preview_options())
            .expect("storyboard renders after accepting caption");

        assert_eq!(rendered.step_count, state.guide.steps().len());
        assert_eq!(
            state.guide.steps()[0].caption,
            "The preferences window is opened for configuration."
        );
    }

    #[test]
    fn accepted_caption_suggestion_is_used_by_issue_pack_input() {
        let mut state = ws(recording_from_frames());
        let proposal = rollshot_action::CaptionProposal::from_agent_drafts(
            rollshot_action::CaptionProposalId(1),
            42,
            &state.guide,
            vec![rollshot_action::CaptionSuggestionDraft {
                step_source: state.guide.steps()[0].source,
                title: Some("Open Preferences".to_string()),
                caption: "The preferences window is opened for configuration.".to_string(),
                confidence: 0.8,
                rationale: None,
            }],
        );
        let _ = update(&mut state, Message::CaptionProposalLoaded(Ok(proposal)));
        let _ = update(
            &mut state,
            Message::AcceptCaptionSuggestion(rollshot_action::CaptionSuggestionId(1)),
        );

        let input = timeline_issue_pack_input(&state);
        let first_step = &input.action_guide.as_ref().unwrap().steps[0];

        assert_eq!(first_step.title, "Open Preferences");
        assert_eq!(
            first_step.caption.as_deref(),
            Some("The preferences window is opened for configuration.")
        );
    }
```

- [ ] **Step 2: Run targeted Action Guide suites**

Run:

```bash
rtk cargo test -p rollshot-action caption
rtk cargo test -p rollshot-action storyboard
rtk cargo test -p rollshot-app --features action-guide timeline_workspace
rtk cargo test -p rollshot-app --features action-guide issue_pack
```

Expected: PASS.

- [ ] **Step 3: Run formatting**

Run:

```bash
rtk cargo fmt --check
```

Expected: PASS.

- [ ] **Step 4: Run clippy if the targeted tests pass**

Run:

```bash
rtk cargo clippy --workspace --all-targets -- -D warnings
```

Expected: PASS. If clippy fails in unrelated pre-existing OCR/default-member configuration, record the exact failure and run the narrower command:

```bash
rtk cargo clippy -p rollshot-action --all-targets -- -D warnings
rtk cargo clippy -p rollshot-app --features action-guide --all-targets -- -D warnings
```

- [ ] **Step 5: Commit**

```bash
rtk git add crates/rollshot-app/src/timeline_workspace/update.rs
rtk git commit -m "test(action): verify caption proposal export flow"
```

---

## Manual Verification

- Build/run `rollshot-app` with `action-guide`.
- Record an Action Guide with at least two steps.
- Click `Suggest Captions` without provider credentials.
- Verify a recoverable inline message appears and no guide fields change.
- Configure a provider through the existing Rollshot config.
- Click `Suggest Captions`.
- Verify suggestions appear as pending cards.
- Accept one suggestion.
- Verify the selected step title/caption fields update immediately.
- Reject another suggestion.
- Verify the corresponding step remains unchanged.
- Export Storyboard.
- Verify accepted captions are visible in the PNG.
- Export Bug Report.
- Verify `issue.md` includes accepted captions and `action-guide/storyboard.png` reflects them.

---

## Out Of Scope Follow-Ups

- P5b: image-aware caption generation using authorized keyframe uploads or vision summaries.
- P5c: agent-proposed number callouts using `rollshot-edit-proposal::EditProposal` wrapped with Action Guide step identity.
- P5d: agent-proposed redactions with explicit Issue Pack redaction-safety copy and optional redacted keyframe asset generation.
- Shared provider configuration extraction out of `result_workspace::workbench` if a third agent surface is added.
