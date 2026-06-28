# Smart Redaction Agent Phase E Improve Existing Preset Loop Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Let users revise an existing Smart Redaction preset from reviewed candidate corrections, producing an immutable child revision after manual review.

**Architecture:** Keep the bounded agent runtime unchanged. The workbench converts review state into privacy-safe correction evidence, starts a normal agent run with `RunKind::Improve`, and carries revision lineage metadata in workbench state until the user saves the improved draft.

> **Note — `RunKind::Improve` is run metadata, not a runtime branch.** `start_agent_run` does not read `params.mode`. Improve behavior is carried by three things only: (1) the correction-evidence user message, (2) the active-revision source seeded as the run's starting source (`active_revision_source`), and (3) the Task 5 system-prompt section. Do not look for `mode`-based branching in the runner — there is none.

**Tech Stack:** Rust, iced task/update state, `rollshot-agent` bounded run loop, `rollshot-preset` immutable revision store, existing Phase D eval harness.

---

## File Structure

- Modify `crates/rollshot-app/src/result_workspace/workbench/review.rs`
  - Owns correction evidence extraction and human-readable improve summaries.
  - Already owns review lowering and revision saving.
  - Task 1 replaces the count-only `CorrectionEvidence` struct, its `Display`
    impl, and `assemble_correction_evidence`, and deletes the two obsolete
    count-based tests.
- Modify `crates/rollshot-app/src/result_workspace/workbench/state.rs`
  - Adds run-lineage fields to `RunState::Running` (the enum is defined here,
    re-exported via `mod.rs`).
- Modify `crates/rollshot-app/src/result_workspace/workbench/mod.rs`
  - Adds run-lineage fields to `PendingRunParams` and `PendingDraft`.
- Modify `crates/rollshot-app/src/result_workspace/workbench/view.rs`
  - Rebuilds the improve-modal `CorrectionEvidence` via
    `assemble_correction_evidence` (the manual count-field construction no
    longer compiles after Task 1), and adds the review-bar
    "Ask agent to revise" entry point wired to `AskAgentToRevise`.
- Modify `crates/rollshot-app/src/result_workspace/update.rs`
  - Wires `AskAgentToRevise`, carries lineage metadata through
    `DisclosureConfirmed`/`RunTerminal`, and saves child revisions.
- Modify `crates/rollshot-app/src/result_workspace/workbench/run.rs`
  - Adds deterministic tests around improve parameter assembly, lineage
    threading, and terminal draft metadata.
- Modify `crates/rollshot-agent/src/driver.rs`
  - Updates the system prompt and adds a prompt contract test for improve
    semantics.
- Modify `docs/smart-redaction-eval.md`
  - Notes that live improve cassettes are deferred until Phase E prompt stabilizes.

## Task 1: Correction Evidence Model

**Files:**
- Modify: `crates/rollshot-app/src/result_workspace/workbench/review.rs`
- Modify: `crates/rollshot-app/src/result_workspace/workbench/view.rs`

- [ ] **Step 1: Add failing tests for rejected, resized, and manual-added evidence**

Append these tests inside the existing `evidence_tests` module in `review.rs`:

```rust
fn agent_candidate(id: u64, label: &str, bounds: ImageRect) -> ProposedCandidate {
    ProposedCandidate {
        id: CandidateId(id),
        edit: ProposedEdit::AddRedaction { bounds },
        confidence: 0.9,
        label: label.into(),
        rationale: None,
        provenance: Provenance {
            source: ProvenanceSource::Agent { run_id: 7 },
        },
    }
}

fn manual_candidate(id: u64, bounds: ImageRect) -> ProposedCandidate {
    ProposedCandidate {
        id: CandidateId(id),
        edit: ProposedEdit::AddRedaction { bounds },
        confidence: 1.0,
        label: "manual".into(),
        rationale: Some("Manually added missing candidate".into()),
        provenance: Provenance {
            source: ProvenanceSource::Manual,
        },
    }
}

#[test]
fn correction_evidence_records_rejected_resized_and_manual_added_bounds() {
    let original_a = ImageRect { x: 0.0, y: 0.0, width: 10.0, height: 10.0 };
    let original_b = ImageRect { x: 20.0, y: 20.0, width: 10.0, height: 10.0 };
    let corrected_b = ImageRect { x: 22.0, y: 18.0, width: 14.0, height: 12.0 };
    let manual = ImageRect { x: 80.0, y: 10.0, width: 12.0, height: 8.0 };
    let p = EditProposal {
        id: ProposalId(1),
        base_document_state_id: 0,
        candidates: vec![
            agent_candidate(1, "email", original_a),
            agent_candidate(2, "name", original_b),
            manual_candidate(3, manual),
        ],
        confidence_summary: ConfidenceSummary::from_confidences(&[0.9, 0.9, 1.0]),
        rationale_summary: None,
        provenance: Provenance { source: ProvenanceSource::Agent { run_id: 7 } },
    };
    let mut review = super::super::state::CandidateReview::from_candidates(&[
        CandidateId(1),
        CandidateId(2),
        CandidateId(3),
    ]);
    review.mark_rejected(CandidateId(1));
    review.mark_modified(CandidateId(2), ProposedEdit::AddRedaction { bounds: corrected_b });
    review.mark_modified(CandidateId(3), ProposedEdit::AddRedaction { bounds: manual });

    let e = assemble_correction_evidence(&p, &review);
    assert_eq!(e.rejected.len(), 1);
    assert_eq!(e.resized.len(), 1);
    assert_eq!(e.manual_added.len(), 1);
    assert_eq!(e.rejected[0].original_bounds, original_a);
    assert_eq!(e.resized[0].original_bounds, original_b);
    assert_eq!(e.resized[0].corrected_bounds, corrected_b);
    assert_eq!(e.manual_added[0].bounds, manual);
    assert!(!e.is_empty());
}

#[test]
fn correction_evidence_agent_message_is_deterministic_and_privacy_safe() {
    let original = ImageRect { x: 0.0, y: 0.0, width: 10.0, height: 10.0 };
    let p = EditProposal {
        id: ProposalId(1),
        base_document_state_id: 0,
        candidates: vec![agent_candidate(1, "email", original)],
        confidence_summary: ConfidenceSummary::from_confidences(&[0.9]),
        rationale_summary: None,
        provenance: Provenance { source: ProvenanceSource::Agent { run_id: 7 } },
    };
    let mut review = super::super::state::CandidateReview::from_candidates(&[CandidateId(1)]);
    review.mark_rejected(CandidateId(1));

    let e = assemble_correction_evidence(&p, &review);
    let msg = e.to_agent_message();
    assert!(msg.contains("Rejected false positives"));
    assert!(msg.contains("id=1 label=email"));
    assert!(msg.contains("x=0.0 y=0.0 w=10.0 h=10.0"));
    assert!(!msg.contains("data:image"));
    assert!(!msg.contains("authorization"));
}
```

- [ ] **Step 2: Run the focused tests and verify they fail**

Run:

```bash
rtk cargo test -p rollshot-app workbench::review::evidence_tests -- --nocapture
```

Expected: fail because `CorrectionEvidence` has no `rejected`, `resized`, `manual_added`, `is_empty`, or `to_agent_message` fields/methods.

- [ ] **Step 3: Implement correction evidence structs and extraction**

In `review.rs`, replace the current `CorrectionEvidence` struct and `assemble_correction_evidence` with:

```rust
use rollshot_edit_proposal::{CandidateId, EditProposal, ProposedEdit, ProvenanceSource};
use rollshot_image_document::ImageRect;

#[derive(Debug, Clone, PartialEq)]
pub struct RejectedCorrection {
    pub id: CandidateId,
    pub label: String,
    pub original_bounds: ImageRect,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ResizedCorrection {
    pub id: CandidateId,
    pub label: String,
    pub original_bounds: ImageRect,
    pub corrected_bounds: ImageRect,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ManualAddedCorrection {
    pub id: CandidateId,
    pub bounds: ImageRect,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct CorrectionEvidence {
    pub accepted_count: usize,
    pub rejected: Vec<RejectedCorrection>,
    pub resized: Vec<ResizedCorrection>,
    pub manual_added: Vec<ManualAddedCorrection>,
}

fn rect_summary(bounds: ImageRect) -> String {
    format!(
        "x={:.1} y={:.1} w={:.1} h={:.1}",
        bounds.x, bounds.y, bounds.width, bounds.height
    )
}

impl CorrectionEvidence {
    pub fn is_empty(&self) -> bool {
        self.rejected.is_empty() && self.resized.is_empty() && self.manual_added.is_empty()
    }

    pub fn summary_line(&self) -> String {
        format!(
            "{} rejected, {} resized, {} manually added",
            self.rejected.len(),
            self.resized.len(),
            self.manual_added.len()
        )
    }

    pub fn to_agent_message(&self) -> String {
        let mut out = String::from(
            "Improve the current Smart Redaction detector using this reviewed evidence.\n\
             Preserve existing useful detections, remove overfires, and add missed targets.\n\n\
             Correction evidence:\n",
        );
        out.push_str(&format!("- Summary: {}\n", self.summary_line()));
        if !self.rejected.is_empty() {
            out.push_str("- Rejected false positives:\n");
            for r in &self.rejected {
                out.push_str(&format!(
                    "  - id={} label={} original={}\n",
                    r.id.0,
                    r.label,
                    rect_summary(r.original_bounds)
                ));
            }
        }
        if !self.resized.is_empty() {
            out.push_str("- Resized target corrections:\n");
            for r in &self.resized {
                out.push_str(&format!(
                    "  - id={} label={} original={} corrected={}\n",
                    r.id.0,
                    r.label,
                    rect_summary(r.original_bounds),
                    rect_summary(r.corrected_bounds)
                ));
            }
        }
        if !self.manual_added.is_empty() {
            out.push_str("- Manually added missed targets:\n");
            for m in &self.manual_added {
                out.push_str(&format!("  - id={} bounds={}\n", m.id.0, rect_summary(m.bounds)));
            }
        }
        out
    }
}

impl std::fmt::Display for CorrectionEvidence {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.summary_line())
    }
}

pub fn assemble_correction_evidence(
    proposal: &EditProposal,
    review: &super::state::CandidateReview,
) -> CorrectionEvidence {
    let (accepted_ids, rejected_ids, modified_pairs) = review.decision_sets();
    let mut evidence = CorrectionEvidence {
        accepted_count: accepted_ids.len(),
        ..CorrectionEvidence::default()
    };

    for id in rejected_ids {
        if let Some(candidate) = proposal.candidates.iter().find(|c| c.id == id) {
            if let Some(original_bounds) = super::state::proposed_edit_bounds(&candidate.edit) {
                evidence.rejected.push(RejectedCorrection {
                    id,
                    label: candidate.label.clone(),
                    original_bounds,
                });
            }
        }
    }

    for (id, corrected_edit) in modified_pairs {
        let Some(corrected_bounds) = super::state::proposed_edit_bounds(&corrected_edit) else {
            continue;
        };
        let Some(candidate) = proposal.candidates.iter().find(|c| c.id == id) else {
            continue;
        };
        if matches!(candidate.provenance.source, ProvenanceSource::Manual) {
            evidence.manual_added.push(ManualAddedCorrection { id, bounds: corrected_bounds });
            continue;
        }
        if let Some(original_bounds) = super::state::proposed_edit_bounds(&candidate.edit) {
            evidence.resized.push(ResizedCorrection {
                id,
                label: candidate.label.clone(),
                original_bounds,
                corrected_bounds,
            });
        }
    }

    evidence
}
```

Import scope: `review.rs` already does `use rollshot_edit_proposal::{lower, EditProposal, ReviewDecision};` — **merge** `CandidateId` and `ProvenanceSource` into that line; add `use rollshot_image_document::ImageRect;`. Do **not** import `ProposedEdit` at module scope — it is not named in the new code and would warn under `-D warnings`. The new struct drops the speculative `accepted_count` field (nothing reads it); if you keep it, justify the reader.

This step **replaces three things** in `review.rs`: the `CorrectionEvidence` struct, its existing `Display` impl (the old one references the removed count fields and would either conflict or fail to compile), and `assemble_correction_evidence`.

- [ ] **Step 3b: Delete the obsolete count-based tests**

In `review.rs` `evidence_tests`, delete `correction_evidence_counts_reject_and_modify` and `correction_evidence_all_pending_is_zero`. They reference the removed `rejected_count`/`modified_count`/`added_count` fields, and their semantics are now wrong (a `Manual`-provenance candidate marked modified is classified as `manual_added`, not `resized`). The new Step 1 tests supersede them.

- [ ] **Step 3c: Fix the `view.rs` evidence construction (D1 — crate won't build otherwise)**

In `view.rs` (~line 56), the improve-modal branch builds `CorrectionEvidence { rejected_count, modified_count, added_count }` from `wb.review`. Those fields no longer exist. Replace the manual construction with the shared extractor:

```rust
let evidence = match wb.pending_proposal.as_ref() {
    Some(proposal) => super::review::assemble_correction_evidence(proposal, &wb.review),
    None => super::review::CorrectionEvidence::default(),
};
improve_modal(&evidence)
```

`improve_modal`'s `text(format!("- {evidence}"))` keeps working via the new `Display`/`summary_line`. No other `view.rs` change is needed in Task 1 (the entry-point button is Task 3).

- [ ] **Step 4: Build the crate and run the focused tests, verify they pass**

Run:

```bash
rtk cargo check -p rollshot-app
rtk cargo test -p rollshot-app workbench::review::evidence_tests -- --nocapture
```

Expected: the whole crate compiles (proves `view.rs` was updated) and all evidence tests pass.

- [ ] **Step 5: Commit Task 1**

Run:

```bash
rtk git add crates/rollshot-app/src/result_workspace/workbench/review.rs crates/rollshot-app/src/result_workspace/workbench/view.rs
rtk git commit -m "feat(app): model smart redaction correction evidence"
```

## Task 2: Run Lineage State

**Files:**
- Modify: `crates/rollshot-app/src/result_workspace/workbench/state.rs`
- Modify: `crates/rollshot-app/src/result_workspace/workbench/mod.rs`
- Modify: `crates/rollshot-app/src/result_workspace/update.rs`
- Modify: `crates/rollshot-app/src/result_workspace/workbench/run.rs`

- [ ] **Step 1: Add failing compile-time usage for lineage fields**

`RunState` is defined in `state.rs` (and re-exported from `mod.rs`);
`PendingDraft`/`PendingRunParams` are defined in `mod.rs`. Edit each in its
real home. This intentionally causes compile errors until `update.rs` is wired.

In `state.rs`, update `RunState` (note `Option<rollshot_preset::RevisionId>`
requires `rollshot_preset` to be a dependency of `rollshot-app` — it already is;
reference it by full path here):

```rust
#[derive(Debug, Clone, Default)]
pub enum RunState {
    #[default]
    Idle,
    Running {
        cancellation: RunCancellation,
        parent_revision_id: Option<rollshot_preset::RevisionId>,
        revision_note: Option<String>,
    },
    Terminal(RunTerminalState),
}
```

In `mod.rs`, update the two struct definitions:

```rust
#[derive(Debug, Clone)]
pub struct PendingDraft {
    pub source: String,
    pub assistant_text: String,
    pub validation_summary: rollshot_automation::ValidationSummary,
    pub parent_revision_id: Option<rollshot_preset::RevisionId>,
    pub revision_note: Option<String>,
}

#[derive(Debug, Clone)]
pub struct PendingRunParams {
    pub user_message: String,
    pub image_dims: (u32, u32),
    pub active_revision_source: Option<String>,
    pub mode: RunKind,
    pub parent_revision_id: Option<rollshot_preset::RevisionId>,
    pub revision_note: Option<String>,
}
```

- [ ] **Step 2: Run check and verify failures point to missing field initializers**

Run:

```bash
rtk cargo check -p rollshot-app
```

Expected: fail at `RunState::Running`, `PendingDraft`, and `PendingRunParams` initializers.

- [ ] **Step 3: Wire author-run defaults**

In `update.rs`, update the `SendRequested` author params:

```rust
let params = super::workbench::PendingRunParams {
    user_message,
    image_dims: (w, h),
    active_revision_source: workbench
        .active_revision
        .as_ref()
        .map(|r| r.artifact.source.clone()),
    mode: super::workbench::RunKind::Author,
    parent_revision_id: None,
    revision_note: None,
};
```

In `DisclosureConfirmed`, before calling `start_agent_run`, preserve lineage values:

```rust
let parent_revision_id = params.parent_revision_id.clone();
let revision_note = params.revision_note.clone();
```

Then update the running state assignment:

```rust
workbench.run_state = super::workbench::RunState::Running {
    cancellation,
    parent_revision_id,
    revision_note,
};
```

In `RunTerminal`, capture lineage before replacing the running state:

```rust
let (parent_revision_id, revision_note) = match &workbench.run_state {
    super::workbench::RunState::Running {
        parent_revision_id,
        revision_note,
        ..
    } => (parent_revision_id.clone(), revision_note.clone()),
    _ => (None, None),
};
```

Then update `PendingDraft` construction:

```rust
workbench.pending_draft = Some(super::workbench::PendingDraft {
    source: ready.automation.source.clone(),
    assistant_text: ready.assistant_text.clone(),
    validation_summary: ready.automation.validation_summary.clone(),
    parent_revision_id,
    revision_note,
});
```

- [ ] **Step 4: Update existing tests and helper initializers**

Search for `PendingRunParams {` and `RunState::Running {`:

```bash
rtk rg -n "PendingRunParams \\{|RunState::Running \\{" crates/rollshot-app/src/result_workspace
```

Add `parent_revision_id: None` and `revision_note: None` to **every**
`RunState::Running { … }` and `PendingRunParams { … }` *construction* site (not
the `RunState::Running { .. }` pattern matches, which already use `..`). This
includes the three test constructions in `run.rs` (`cancel_run_calls_cancellation`,
`run_failed_sets_error_and_terminal`, `disclosure_confirmed_blocked_while_running`)
and the `disclosure_cancelled_clears_pending_run_and_flag` test — none of which
"start an author run" but all of which construct the affected types.

- [ ] **Step 4b: Add a failing test for lineage threading Running → Terminal → PendingDraft**

This is the hop Task 2 actually adds (Step 3's `RunTerminal` capture) and is
otherwise untested. In `run.rs` `reducer_tests`, add a test that seeds a
`Running` state carrying lineage, fires `RunTerminal(ReadyForReview(..))`, and
asserts the lineage lands on `PendingDraft`. Reuse the existing
`ready_for_review_with_text` helper:

```rust
#[test]
fn run_terminal_carries_lineage_into_pending_draft() {
    let mut ws = ws_with_workbench();
    wb_mut(&mut ws).run_state = super::super::RunState::Running {
        cancellation: rollshot_agent::runtime::RunCancellation::new(),
        parent_revision_id: Some(rollshot_preset::RevisionId("rev-parent".into())),
        revision_note: Some("improved from rev-parent; 1 rejected, 0 resized, 0 manually added".into()),
    };
    let ready = ready_for_review_with_text("done");
    let _ = update(
        &mut ws,
        Message::Workbench(WorkbenchMessage::RunTerminal(
            RunTerminalState::ReadyForReview(Box::new(ready)),
        )),
    );
    let draft = wb(&ws).pending_draft.as_ref().expect("draft populated");
    assert_eq!(draft.parent_revision_id.as_ref().unwrap().0, "rev-parent");
    assert!(draft.revision_note.as_ref().unwrap().contains("1 rejected"));
}
```

Because Task 2 bundles the `RunTerminal` lineage capture into Step 3, this test
passes once Step 3 is in place. To confirm it is a real guard (not a tautology),
temporarily comment out the `parent_revision_id`/`revision_note` assignment in
the Step 3 `PendingDraft` construction and watch it go RED, then restore:

```bash
rtk cargo test -p rollshot-app run_terminal_carries_lineage_into_pending_draft -- --nocapture
```

- [ ] **Step 5: Run check and focused tests**

Run:

```bash
rtk cargo check -p rollshot-app
rtk cargo test -p rollshot-app result_workspace::workbench::run
```

Expected: check passes and existing workbench run tests pass.

- [ ] **Step 6: Commit Task 2**

Run:

```bash
rtk git add crates/rollshot-app/src/result_workspace/workbench/state.rs crates/rollshot-app/src/result_workspace/workbench/mod.rs crates/rollshot-app/src/result_workspace/update.rs crates/rollshot-app/src/result_workspace/workbench/run.rs
rtk git commit -m "feat(app): carry smart redaction run lineage"
```

## Task 3: Wire AskAgentToRevise

**Files:**
- Modify: `crates/rollshot-app/src/result_workspace/update.rs`
- Modify: `crates/rollshot-app/src/result_workspace/workbench/run.rs`
- Modify: `crates/rollshot-app/src/result_workspace/workbench/view.rs`

- [ ] **Step 1: Add failing update tests for improve param assembly**

In the existing `reducer_tests` module in `workbench/run.rs`, add this helper
next to the existing `candidate` helper:

```rust
fn agent_candidate(id: u64, b: ImageRect) -> ProposedCandidate {
    ProposedCandidate {
        id: CandidateId(id),
        edit: ProposedEdit::AddRedaction { bounds: b },
        confidence: 0.9,
        label: "agent".into(),
        rationale: None,
        provenance: Provenance {
            source: ProvenanceSource::Agent { run_id: 7 },
        },
    }
}
```

Add this helper next to `ready_for_review_with_text`:

```rust
fn active_revision_for_reducer_test() -> rollshot_preset::AutomationRevision {
    use rollshot_preset::{
        AutomationRevision, PresetId, RevisionId, RevisionOrigin, RevisionProvenance,
        STORE_SCHEMA_VERSION,
    };
    let source = "function main(input) { return { candidates: [] }; }";
    let validated = rollshot_automation::validate_source(
        source,
        &rollshot_automation::ValidationLimits::default(),
    )
    .unwrap();
    AutomationRevision {
        store_schema_version: STORE_SCHEMA_VERSION,
        id: RevisionId("rev-1".into()),
        preset_id: PresetId("workbench-draft".into()),
        parent_id: None,
        created_at: "2026-06-28T00:00:00Z".into(),
        provenance: RevisionProvenance {
            origin: RevisionOrigin::AgentRun,
            note: None,
            source_run_ref: Some("7".into()),
        },
        artifact: validated,
    }
}
```

Add this helper next to `wb_mut`:

```rust
fn seed_active_revision_pending_proposal_and_rejection(ws: &mut ResultWorkspace) {
    let wb = wb_mut(ws);
    wb.active_revision = Some(active_revision_for_reducer_test());
    let p = proposal(vec![agent_candidate(1, rect(10.0, 10.0, 50.0, 50.0))]);
    wb.pending_proposal = Some(p);
    wb.review = super::super::CandidateReview::from_candidates(&[CandidateId(1)]);
    wb.review.mark_rejected(CandidateId(1));
}
```

Add this test near the existing workbench message tests:

```rust
#[test]
fn ask_agent_to_revise_queues_improve_run_with_correction_evidence() {
    let mut ws = ws_with_workbench();
    seed_active_revision_pending_proposal_and_rejection(&mut ws);

    let task = update(
        &mut ws,
        Message::Workbench(WorkbenchMessage::AskAgentToRevise),
    );
    drop(task);

    let state = wb(&ws);
    let params = state.pending_run.as_ref().expect("pending improve run");
    assert_eq!(params.mode, super::super::RunKind::Improve);
    assert!(params.user_message.contains("Rejected false positives"));
    assert!(params.active_revision_source.as_ref().unwrap().contains("function main"));
    assert_eq!(params.parent_revision_id.as_ref().unwrap().0, "rev-1");
    assert!(params.revision_note.as_ref().unwrap().contains("1 rejected"));
    assert!(state.disclosure_pending);
}

#[test]
fn ask_agent_to_revise_is_noop_without_corrections() {
    let mut ws = ws_with_workbench();
    // Active revision + proposal present, but the review has no rejections,
    // resizes, or manual additions → empty evidence → silent no-op.
    // Scope the mutable borrow in a block so the local does not shadow the
    // `wb(&ws)` accessor used below.
    {
        let wb = wb_mut(&mut ws);
        wb.active_revision = Some(active_revision_for_reducer_test());
        wb.pending_proposal = Some(proposal(vec![agent_candidate(1, rect(10.0, 10.0, 50.0, 50.0))]));
        wb.review = super::super::CandidateReview::from_candidates(&[CandidateId(1)]);
    }

    let _ = update(
        &mut ws,
        Message::Workbench(WorkbenchMessage::AskAgentToRevise),
    );
    let state = wb(&ws);
    assert!(state.pending_run.is_none(), "no run queued without corrections");
    assert!(!state.disclosure_pending, "disclosure not opened");
}
```

- [ ] **Step 2: Run the failing test**

Run:

```bash
rtk cargo test -p rollshot-app ask_agent_to_revise_queues_improve_run_with_correction_evidence -- --nocapture
```

Expected: fail because `AskAgentToRevise` is still a no-op.

- [ ] **Step 3: Implement `AskAgentToRevise`**

Replace the no-op arm in `update.rs` with:

```rust
super::workbench::WorkbenchMessage::AskAgentToRevise => {
    if workbench.run_state.is_running() {
        return Task::none();
    }
    let Some(active_revision) = workbench.active_revision.as_ref() else {
        return Task::none();
    };
    let Some(proposal) = workbench.pending_proposal.as_ref() else {
        return Task::none();
    };
    let evidence =
        super::workbench::review::assemble_correction_evidence(proposal, &workbench.review);
    if evidence.is_empty() {
        return Task::none();
    }
    let (w, h) = state.document.image.source().dimensions();
    let summary = evidence.summary_line();
    let params = super::workbench::PendingRunParams {
        user_message: evidence.to_agent_message(),
        image_dims: (w, h),
        active_revision_source: Some(active_revision.artifact.source.clone()),
        mode: super::workbench::RunKind::Improve,
        parent_revision_id: Some(active_revision.id.clone()),
        revision_note: Some(format!("improved from {}; {summary}", active_revision.id.0)),
    };
    workbench.disclosure_pending = true;
    workbench.pending_run = Some(params);
    Task::none()
}
```

Keep `DiscardDraft`, `DiscardCandidates`, `ToggleAdvancedDetails`, `OpenProviderSettings`, and `DisclosureRequested(_)` in a separate no-op arm.

- [ ] **Step 3b: Add the UI entry point (without it the improve loop is unreachable)**

The reducer logic above is dead until a control emits `AskAgentToRevise`. Today
the only improve affordances (`view.rs` "Improve preset" buttons) emit `ImStart`,
which opens the inert `improve_modal` (its "Send improvement" button has no
`on_press`). Add a button in the **candidate review bar** (near the existing
"Apply" control, `view.rs` ~lines 180-250), enabled only when there is something
to send:

```rust
// Mirror the reducer guard: needs an active revision to revise *from*, a
// proposal, and at least one correction. Otherwise the click is a no-op.
let revise_enabled = wb.active_revision.is_some()
    && wb
        .pending_proposal
        .as_ref()
        .map(|p| !super::review::assemble_correction_evidence(p, &wb.review).is_empty())
        .unwrap_or(false);
// …in the review bar row…
button(text("Ask agent to revise")).on_press_maybe(
    revise_enabled.then_some(Message::Workbench(WorkbenchMessage::AskAgentToRevise)),
)
```

When fired, `AskAgentToRevise` sets `pending_run`, so the view shows the working
`disclosure_modal` (confirm → `DisclosureConfirmed` → real Improve run), not the
inert `improve_modal`. The legacy `ImStart`/`improve_modal` stub is now
superseded; removing it is left as a follow-up (out of scope here) to keep this
task's diff focused.

- [ ] **Step 4: Run the focused improve tests**

Run:

```bash
rtk cargo check -p rollshot-app
rtk cargo test -p rollshot-app ask_agent_to_revise_queues_improve_run_with_correction_evidence -- --nocapture
rtk cargo test -p rollshot-app ask_agent_to_revise_is_noop_without_corrections -- --nocapture
rtk cargo test -p rollshot-app workbench::review::evidence_tests -- --nocapture
```

Expected: crate compiles (view button wired) and all three test invocations pass.

- [ ] **Step 5: Commit Task 3**

Run:

```bash
rtk git add crates/rollshot-app/src/result_workspace/update.rs crates/rollshot-app/src/result_workspace/workbench/run.rs crates/rollshot-app/src/result_workspace/workbench/view.rs
rtk git commit -m "feat(app): start smart redaction improve runs"
```

## Task 4: Save Improved Drafts as Child Revisions

**Files:**
- Modify: `crates/rollshot-app/src/result_workspace/workbench/review.rs`
- Modify: `crates/rollshot-app/src/result_workspace/update.rs`

- [ ] **Step 1: Add failing save test for parent and provenance note**

In `review.rs` `save_tests`, add:

```rust
#[test]
fn save_revision_records_parent_and_note() {
    let tmp = tempfile::tempdir().unwrap();
    let store = PresetStore::open(tmp.path().to_path_buf());
    let preset_id = PresetId("test-preset".into());
    store
        .create_preset(
            preset_id.clone(),
            "Test".into(),
            "intent".into(),
            "2026-01-01T00:00:00Z".into(),
        )
        .unwrap();
    let source = r#"function main(input) { return { candidates: [] }; }"#;
    let parent = rollshot_preset::RevisionId("rev-parent".into());
    save_revision(
        &store,
        &preset_id,
        source,
        Some(&parent),
        Some("improved from rev-parent; 1 rejected, 0 resized, 0 manually added"),
        42,
        "2026-01-01T00:00:00Z".into(),
    )
    .unwrap();
    let active = store.load_active_revision(&preset_id).unwrap();
    assert_eq!(active.parent_id, Some(parent));
    assert_eq!(
        active.provenance.note.as_deref(),
        Some("improved from rev-parent; 1 rejected, 0 resized, 0 manually added")
    );
}
```

- [ ] **Step 2: Run the failing save tests**

Run:

```bash
rtk cargo test -p rollshot-app workbench::review::save_tests -- --nocapture
```

Expected: fail because `save_revision` does not accept a note argument.

- [ ] **Step 3: Extend `save_revision`**

Change the signature in `review.rs`:

```rust
pub fn save_revision(
    store: &PresetStore,
    preset_id: &PresetId,
    source: &str,
    parent_rev_id: Option<&RevisionId>,
    provenance_note: Option<&str>,
    session_id: u64,
    now: String,
) -> Result<(), WorkbenchError> {
```

Change provenance construction:

```rust
let provenance = RevisionProvenance {
    origin: RevisionOrigin::AgentRun,
    note: provenance_note.map(str::to_owned),
    source_run_ref: Some(session_id.to_string()),
};
```

Update existing tests and call sites by passing `None` where no note is expected.

- [ ] **Step 4: Wire `SavePresetOrRevision` to draft lineage**

In `update.rs`, change the save call:

```rust
match super::workbench::review::save_revision(
    &store,
    &preset_id,
    &draft.source,
    draft.parent_revision_id.as_ref(),
    draft.revision_note.as_deref(),
    workbench.session.session_id.get(),
    chrono::Utc::now().to_rfc3339(),
) {
```

- [ ] **Step 5: Run save tests**

Run:

```bash
rtk cargo test -p rollshot-app workbench::review::save_tests -- --nocapture
```

Expected: pass.

- [ ] **Step 6: Commit Task 4**

Run:

```bash
rtk git add crates/rollshot-app/src/result_workspace/workbench/review.rs crates/rollshot-app/src/result_workspace/update.rs
rtk git commit -m "feat(app): save improved presets as child revisions"
```

## Task 5: Improve Prompt Contract

**Files:**
- Modify: `crates/rollshot-agent/src/driver.rs`

- [ ] **Step 1: Add a failing prompt contract test**

There is **no** existing test whose name contains `system_prompt`, and no test
currently binds a `system_prompt` local to assert `.contains(...)` (the closest,
`smart_redaction_prompt_examples_validate`, only validates the embedded JS
examples). So **add a new test** — its name must contain `system_prompt` so the
`-p rollshot-agent system_prompt` filter selects it (otherwise Step 2 runs zero
tests and passes vacuously). Place it in the same `#[cfg(test)]` module as
`smart_redaction_prompt_examples_validate`:

```rust
#[test]
fn smart_redaction_system_prompt_documents_improve_runs() {
    let system_prompt = SMART_REDACTION_SYSTEM_PROMPT;
    assert!(
        system_prompt.contains("Improve runs"),
        "system prompt should document improve runs, got: {:?}",
        system_prompt
    );
    assert!(
        system_prompt.contains("Treat rejected candidates as false positives"),
        "system prompt should explain rejected correction semantics, got: {:?}",
        system_prompt
    );
    assert!(
        system_prompt.contains("Treat manually added candidates as missed targets"),
        "system prompt should explain manual correction semantics, got: {:?}",
        system_prompt
    );
    assert!(
        system_prompt.contains("Explain what changed in the detector before submit_for_review"),
        "system prompt should require detector-change explanation, got: {:?}",
        system_prompt
    );
}
```

- [ ] **Step 2: Run the failing prompt test and confirm it actually runs**

Run:

```bash
rtk cargo test -p rollshot-agent system_prompt -- --nocapture
```

Expected: **1 test runs and fails** on the new assertions. If the output says
`0 tests`, the test name is wrong — fix it before proceeding (a passing-on-zero
run is a false green). The appended prompt section comes after `Authoring loop:`,
so it does not disturb `smart_redaction_prompt_examples_validate`, whose example
extraction ends at the `Authoring loop:` marker.

- [ ] **Step 3: Update the system prompt**

In `SMART_REDACTION_SYSTEM_PROMPT`, after the `Authoring loop` section, add:

```text

Improve runs:
1. The user message may contain reviewed correction evidence from a previous detector run.
2. Treat rejected candidates as false positives to remove or narrow.
3. Treat resized candidates as geometry corrections for the intended target.
4. Treat manually added candidates as missed targets the detector should learn to include.
5. Preserve unrelated useful detections from the current source.
6. Explain what changed in the detector before submit_for_review.
```

- [ ] **Step 4: Run prompt tests**

Run:

```bash
rtk cargo test -p rollshot-agent system_prompt -- --nocapture
```

Expected: pass.

- [ ] **Step 5: Commit Task 5**

Run:

```bash
rtk git add crates/rollshot-agent/src/driver.rs
rtk git commit -m "feat(agent): teach smart redaction improve runs"
```

## Task 6: Deterministic Miss and Overfire Coverage

**Files:**
- Modify: `crates/rollshot-app/src/result_workspace/workbench/review.rs`
- Modify: `docs/smart-redaction-eval.md`

> These two tests partially overlap the Task 1 evidence tests (rejected →
> "Rejected false positives"; manual → "Manually added missed targets"). They
> are kept intentionally as named overfire/miss documentation of the two
> correction modes (per the project's "rather too many tests than too few"
> preference). No `run.rs` change happens in this task.

- [ ] **Step 1: Add overfire and miss evidence tests**

In `review.rs` `evidence_tests`, add:

```rust
#[test]
fn rejected_candidate_formats_as_overfire_feedback() {
    let bounds = ImageRect { x: 4.0, y: 5.0, width: 6.0, height: 7.0 };
    let p = EditProposal {
        id: ProposalId(1),
        base_document_state_id: 0,
        candidates: vec![agent_candidate(1, "url-bar", bounds)],
        confidence_summary: ConfidenceSummary::from_confidences(&[0.9]),
        rationale_summary: None,
        provenance: Provenance { source: ProvenanceSource::Agent { run_id: 7 } },
    };
    let mut review = super::super::state::CandidateReview::from_candidates(&[CandidateId(1)]);
    review.mark_rejected(CandidateId(1));
    let msg = assemble_correction_evidence(&p, &review).to_agent_message();
    assert!(msg.contains("Rejected false positives"));
    assert!(msg.contains("label=url-bar"));
}

#[test]
fn manual_candidate_formats_as_missed_target_feedback() {
    let bounds = ImageRect { x: 44.0, y: 55.0, width: 66.0, height: 77.0 };
    let p = EditProposal {
        id: ProposalId(1),
        base_document_state_id: 0,
        candidates: vec![manual_candidate(9, bounds)],
        confidence_summary: ConfidenceSummary::from_confidences(&[1.0]),
        rationale_summary: None,
        provenance: Provenance { source: ProvenanceSource::Agent { run_id: 7 } },
    };
    let mut review = super::super::state::CandidateReview::from_candidates(&[CandidateId(9)]);
    review.mark_modified(CandidateId(9), ProposedEdit::AddRedaction { bounds });
    let msg = assemble_correction_evidence(&p, &review).to_agent_message();
    assert!(msg.contains("Manually added missed targets"));
    assert!(msg.contains("id=9"));
}
```

- [ ] **Step 2: Add deterministic improve docs note**

Append to `docs/smart-redaction-eval.md` under "Deferred: six-fixture seeding workflow":

```markdown
## Phase E improve-loop coverage

Phase E does not require live cassette seeding before implementation. The first
gate is deterministic app coverage for two correction modes:

- overfire: rejected candidate evidence is fed into an improve run;
- miss: manually added candidate evidence is fed into an improve run.

Provider-backed improve cassettes should be recorded after the Phase E prompt
and correction-evidence format stabilize.
```

- [ ] **Step 3: Run deterministic tests**

Run:

```bash
rtk cargo test -p rollshot-app workbench::review::evidence_tests -- --nocapture
rtk cargo test -p rollshot-app ask_agent_to_revise_queues_improve_run_with_correction_evidence -- --nocapture
rtk cargo test -p rollshot-app eval
```

Expected: all pass. `eval` should continue to run the existing selftest gate.

- [ ] **Step 4: Commit Task 6**

Run:

```bash
rtk git add crates/rollshot-app/src/result_workspace/workbench/review.rs docs/smart-redaction-eval.md
rtk git commit -m "test(app): cover smart redaction improve corrections"
```

## Task 7: Final Verification

**Files:**
- Verify all changed files.

- [ ] **Step 1: Run app and agent focused tests**

Run:

```bash
rtk cargo test -p rollshot-app workbench::review
rtk cargo test -p rollshot-app result_workspace::workbench::run
rtk cargo test -p rollshot-agent system_prompt
rtk cargo test -p rollshot-agent provider_contract
```

Expected: all pass.

- [ ] **Step 2: Run eval gate**

Run:

```bash
rtk cargo test -p rollshot-app eval
```

Expected: selftest region eval passes; OCR-gated unseeded fixtures are skipped according to current harness rules.

- [ ] **Step 3: Run formatting**

Run:

```bash
rtk cargo fmt --check
```

Expected: pass.

- [ ] **Step 4: Run clippy if code changes touched shared signatures beyond the listed files**

Run:

```bash
rtk cargo clippy --workspace --all-targets -- -D warnings
```

Expected: pass. If this is too slow for the environment, record the reason and the focused test coverage already run.

- [ ] **Step 5: Final commit if Task 7 required cleanup**

Run only if verification caused additional edits:

```bash
rtk git add crates/rollshot-app/src/result_workspace/workbench/review.rs crates/rollshot-app/src/result_workspace/workbench/state.rs crates/rollshot-app/src/result_workspace/workbench/mod.rs crates/rollshot-app/src/result_workspace/workbench/view.rs crates/rollshot-app/src/result_workspace/update.rs crates/rollshot-app/src/result_workspace/workbench/run.rs crates/rollshot-agent/src/driver.rs docs/smart-redaction-eval.md
rtk git commit -m "fix(app): stabilize smart redaction improve loop"
```
