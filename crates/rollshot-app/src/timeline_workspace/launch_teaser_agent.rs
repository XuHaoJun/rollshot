//! Optional repository scope and durable agent proposal for launch teasers.
#![allow(dead_code)]
//!
//! Provides repository-scope selection, agent orchestration for
//! `suggest_launch_teaser_task`, durable ReadyForReview artifact promotion,
//! restore/review receipt, and field-level proposal state.
//!
//! ```text
//! Idle ──► ScopeSelection { root, entries }
//!    ▲            │ confirmed
//!    │            ▼
//!    │       AgentRunning { operation_id, scope, review }
//!    │            │ finished
//!    │            ▼
//!    └──── ProposalReview { review, proposal }
//!              │ accept/reject
//!              ▼
//!           Applied / Dismissed
//! ```
//!
//! Repository scope is ephemeral per run: the root path and grant are
//! discarded when the run completes or the teaser is closed. Only
//! privacy-safe receipts survive in durable artifacts.

use std::path::PathBuf;

use rollshot_action::launch_teaser::LaunchTeaserPlanV1;
use rollshot_agent::product_task::{
    ArtifactId, ProductArtifactMetadata, ProductTaskId, ProductTaskSnapshot, SourceBinding,
    TaskAttempt, TaskAttemptId, TaskKind,
};

use super::launch_teaser::{LaunchTeaserAgentProposalReview, LaunchTeaserReviewState};

// ========================================================================
// Repository scope
// ========================================================================

/// Maximum number of files/directories in a single grant.
const MAX_GRANT_ENTRIES: usize = 32;

/// Repository scope selection state.
///
/// The user selects a workspace root, then adds files/directories.
/// Entries are stored as normalized relative paths.
#[derive(Debug, Clone)]
pub(crate) struct RepositoryScopeState {
    /// Selected workspace root (absolute, never stored in durable artifacts).
    pub root: PathBuf,
    /// Normalized relative file/directory entries.
    pub entries: Vec<String>,
    /// Whether the scope has been confirmed by the user.
    pub confirmed: bool,
}

impl RepositoryScopeState {
    pub fn new(root: PathBuf) -> Self {
        Self {
            root,
            entries: Vec::new(),
            confirmed: false,
        }
    }

    /// Add a normalized relative entry. Returns an error if the path is
    /// outside the root or exceeds the entry limit.
    pub fn add_entry(&mut self, relative_path: String) -> Result<(), ScopeError> {
        if self.entries.len() >= MAX_GRANT_ENTRIES {
            return Err(ScopeError::TooManyEntries);
        }
        // Validate: no absolute components, no parent-dir traversal.
        let path = std::path::Path::new(&relative_path);
        for comp in path.components() {
            match comp {
                std::path::Component::Normal(_) => {}
                std::path::Component::CurDir => {}
                _ => return Err(ScopeError::InvalidEntry(relative_path)),
            }
        }
        if relative_path.is_empty() {
            return Err(ScopeError::InvalidEntry(relative_path));
        }
        // Deduplicate
        if !self.entries.contains(&relative_path) {
            self.entries.push(relative_path);
            self.entries.sort();
        }
        Ok(())
    }

    /// Remove an entry by index.
    pub fn remove_entry(&mut self, index: usize) {
        if index < self.entries.len() {
            self.entries.remove(index);
        }
    }

    /// Whether the scope is ready for confirmation (at least one entry).
    pub fn can_confirm(&self) -> bool {
        !self.entries.is_empty() && !self.confirmed
    }

    /// Confirm the scope.
    pub fn confirm(&mut self) {
        self.confirmed = true;
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ScopeError {
    InvalidEntry(String),
    TooManyEntries,
}

// ========================================================================
// Agent run state
// ========================================================================

/// States for the agent proposal lifecycle.
#[derive(Debug)]
#[allow(dead_code)]
#[allow(clippy::large_enum_variant)]
pub(crate) enum LaunchTeaserAgentState {
    /// No agent run active.
    Idle,
    /// User is selecting repository scope.
    ScopeSelection(RepositoryScopeState),
    /// Agent run is in flight.
    Running {
        operation_id: u64,
        task_id: ProductTaskId,
        snapshot: ProductTaskSnapshot,
        scope: RepositoryScopeState,
        base_review: LaunchTeaserReviewState,
    },
    /// Proposal is ready for field-level review.
    #[allow(dead_code)]
    ProposalReview {
        task_id: ProductTaskId,
        snapshot: ProductTaskSnapshot,
        proposal: LaunchTeaserAgentProposalReview,
        base_review: LaunchTeaserReviewState,
    },
}

impl LaunchTeaserAgentState {
    pub fn is_idle(&self) -> bool {
        matches!(self, Self::Idle)
    }
}

// ========================================================================
// Durable artifact helpers
// ========================================================================

/// Source binding for a launch teaser agent run.
pub(crate) fn launch_teaser_source_binding(
    project_root_sha256: [u8; 32],
    revision: u64,
    projection_digest: String,
    motion_sha256: String,
) -> SourceBinding {
    SourceBinding::ActionGuideLaunchTeaserProject {
        project_root_sha256,
        revision,
        projection_digest,
        motion_sha256,
    }
}

/// Build a `ProductTaskSnapshot` for a new launch teaser agent task.
pub(crate) fn create_teaser_task_snapshot(
    task_id: ProductTaskId,
    source_binding: SourceBinding,
    now: i64,
) -> Result<ProductTaskSnapshot, String> {
    ProductTaskSnapshot::new(
        task_id,
        TaskKind::ActionGuideLaunchTeaser,
        source_binding,
        now,
    )
    .map_err(|e| e.to_string())
}

/// Start an attempt on a teaser task snapshot.
#[allow(dead_code)]
pub(crate) fn start_teaser_attempt(
    snapshot: &ProductTaskSnapshot,
    run_id: rollshot_agent::domain::RunId,
    now: i64,
) -> Result<ProductTaskSnapshot, String> {
    let attempt = TaskAttempt::new(TaskAttemptId::new(1), run_id, now);
    snapshot
        .start_attempt(attempt, now)
        .map_err(|e| e.to_string())
}

/// Bind a run contract to the running task.
#[allow(dead_code)]
pub(crate) fn bind_teaser_run_contract(
    snapshot: &ProductTaskSnapshot,
    contract: rollshot_agent::product_task::RunContractReceiptV1,
    now: i64,
) -> Result<ProductTaskSnapshot, String> {
    snapshot
        .bind_run_contract(contract, now)
        .map_err(|e| e.to_string())
}

/// Promote a teaser task to ReadyForReview with the given patch payload.
pub(crate) fn promote_teaser_ready_for_review(
    snapshot: &ProductTaskSnapshot,
    patch_json: &[u8],
    provider_id: &str,
    model_id: &str,
    now: i64,
) -> Result<ProductTaskSnapshot, String> {
    let attempt_id = snapshot
        .attempts()
        .last()
        .map(|a| a.attempt_id())
        .ok_or("no active attempt")?;
    let run_id = snapshot
        .attempts()
        .last()
        .map(|a| a.run_id().clone())
        .ok_or("no active attempt")?;

    let patch_hash = {
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(patch_json);
        format!("{:x}", hasher.finalize())
    };

    let metadata = ProductArtifactMetadata::new_v3(
        ArtifactId::parse(format!("artifact-teaser-{}", uuid::Uuid::new_v4()))
            .map_err(|e| e.to_string())?,
        rollshot_agent::product_task::ArtifactRevision::new(1),
        rollshot_agent::product_task::ArtifactKind::ActionGuideLaunchTeaser,
        1,
        patch_hash,
        SourceBinding::ActionGuideLaunchTeaserProject {
            project_root_sha256: [0u8; 32],
            revision: 0,
            projection_digest: String::new(),
            motion_sha256: String::new(),
        },
        snapshot.task_id().clone(),
        attempt_id,
        run_id,
        "launch-teaser-v1".to_string(),
        provider_id.to_string(),
        model_id.to_string(),
        String::new(),
        rollshot_agent::product_task::ArtifactSummary::ActionGuideLaunchTeaser {
            changed_field_count: 0,
            repository_read_count: 0,
        },
        now,
    );

    snapshot
        .record_ready_for_review(metadata, patch_json.to_vec(), None, now)
        .map_err(|e| e.to_string())
}

/// Record a terminal failure on a teaser task.
pub(crate) fn fail_teaser_task(
    snapshot: &ProductTaskSnapshot,
    terminal: rollshot_agent::product_task::TaskTerminal,
    now: i64,
) -> Result<ProductTaskSnapshot, String> {
    snapshot
        .record_terminal(terminal, now)
        .map_err(|e| e.to_string())
}

/// Record cancellation on a teaser task.
pub(crate) fn cancel_teaser_task(
    snapshot: &ProductTaskSnapshot,
    now: i64,
) -> Result<ProductTaskSnapshot, String> {
    snapshot
        .record_terminal(rollshot_agent::product_task::TaskTerminal::Cancelled, now)
        .map_err(|e| e.to_string())
}

// ========================================================================
// Restore
// ========================================================================

/// Try to restore a durable launch teaser proposal from the task store.
///
/// Identity and freshness are checked. Only privacy-safe data is restored.
pub(crate) fn restore_teaser_proposal(
    store: &crate::agent_store::TaskStore,
    source_binding: &SourceBinding,
    now: i64,
) -> Option<(ProductTaskSnapshot, Vec<u8>)> {
    let task = store.reconcile_for_source(source_binding, now).ok()??;
    if task.kind() != TaskKind::ActionGuideLaunchTeaser {
        return None;
    }
    if task.status() != rollshot_agent::product_task::TaskStatus::ReadyForReview {
        return None;
    }
    let payload = task.pending_artifact_payload()?.to_vec();
    Some((task, payload))
}

// ========================================================================
// Patch mapping
// ========================================================================

/// Map an agent patch onto the base plan using the existing review diff logic.
///
/// Returns a field-level proposal review for user acceptance.
pub(crate) fn map_patch_to_review(
    base: &LaunchTeaserPlanV1,
    patch: &rollshot_agent::launch_teaser::LaunchTeaserPatchV1,
) -> Result<LaunchTeaserAgentProposalReview, String> {
    // Build a proposed plan from the base + patch.
    let proposed = apply_patch_to_plan(base, patch)?;
    super::launch_teaser::map_agent_patch(base, &proposed)
}

/// Apply a `LaunchTeaserPatchV1` to a base plan, producing a candidate plan.
fn apply_patch_to_plan(
    base: &LaunchTeaserPlanV1,
    patch: &rollshot_agent::launch_teaser::LaunchTeaserPatchV1,
) -> Result<LaunchTeaserPlanV1, String> {
    let mut candidate = base.clone();

    // Apply hook override.
    if let Some(hook) = &patch.hook {
        candidate.hook = hook.clone();
    }

    // Apply outro override.
    if let Some(outro) = &patch.outro_text {
        candidate.outro_text = outro.clone();
    }

    // Reorder and patch shots.
    if patch.shot_order.len() != base.shots.len() {
        return Err("shot count mismatch".into());
    }

    let mut new_shots = Vec::with_capacity(patch.shot_order.len());
    for &step_id in &patch.shot_order {
        // Find the base shot with this step ID.
        let base_shot = base
            .shots
            .iter()
            .find(|s| s.reviewed_step_id.0 == step_id)
            .ok_or_else(|| format!("step {step_id} not found in base plan"))?;

        let mut shot = base_shot.clone();

        // Apply shot-level patches if present.
        if let Some(patch_shot) = patch.shots.iter().find(|s| s.reviewed_step_id == step_id) {
            if let Some(start) = patch_shot.source_start_ms {
                shot.source_start_ms = start;
            }
            if let Some(end) = patch_shot.source_end_ms {
                shot.source_end_ms = end;
            }
            if let (Some(sx), Some(sy)) = (patch_shot.focus_start_x, patch_shot.focus_start_y) {
                shot.focus_path.start =
                    rollshot_action::launch_teaser::NormalizedPointV1 { x: sx, y: sy };
            }
            if let (Some(ex), Some(ey)) = (patch_shot.focus_end_x, patch_shot.focus_end_y) {
                shot.focus_path.end =
                    rollshot_action::launch_teaser::NormalizedPointV1 { x: ex, y: ey };
            }
            if let Some(z) = patch_shot.zoom_permille {
                shot.focus_path.zoom_permille = z;
            }
            if let Some(s) = patch_shot.speed_permille {
                shot.speed = match s {
                    750 => rollshot_action::launch_teaser::SpeedV1::P750,
                    1000 => rollshot_action::launch_teaser::SpeedV1::P1000,
                    1250 => rollshot_action::launch_teaser::SpeedV1::P1250,
                    1500 => rollshot_action::launch_teaser::SpeedV1::P1500,
                    2000 => rollshot_action::launch_teaser::SpeedV1::P2000,
                    _ => return Err(format!("unsupported speed permille: {s}")),
                };
            }
            if let Some(caption) = &patch_shot.caption {
                shot.caption = caption.clone();
            }
            if let Some(transition) = &patch_shot.transition {
                shot.transition = match transition {
                    rollshot_agent::launch_teaser::LaunchTeaserTransitionPatchV1::Cut => {
                        rollshot_action::launch_teaser::TransitionV1::Cut
                    }
                    rollshot_agent::launch_teaser::LaunchTeaserTransitionPatchV1::Crossfade {
                        duration_ms,
                    } => rollshot_action::launch_teaser::TransitionV1::Crossfade {
                        duration_ms: *duration_ms,
                    },
                };
            }
        }

        new_shots.push(shot);
    }

    candidate.shots = new_shots;

    // Validate the result.
    candidate
        .validate()
        .map_err(|e| format!("patch validation failed: {}", e.category()))?;

    Ok(candidate)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::timeline_workspace::launch_teaser::FieldDecision;
    use rollshot_action::launch_teaser::*;
    use rollshot_action::project::ProjectStepId;

    fn valid_plan_fixture() -> LaunchTeaserPlanV1 {
        fn shot(id: u64, start: u64, end: u64) -> LaunchTeaserShotV1 {
            LaunchTeaserShotV1 {
                reviewed_step_id: ProjectStepId(id),
                source_start_ms: start,
                source_end_ms: end,
                focus_path: FocusPathV1 {
                    start: NormalizedPointV1 { x: 5_000, y: 5_000 },
                    end: NormalizedPointV1 { x: 5_000, y: 5_000 },
                    zoom_permille: 1_000,
                },
                speed: SpeedV1::P1000,
                caption: format!("Step {id}"),
                transition: TransitionV1::Cut,
            }
        }

        LaunchTeaserPlanV1 {
            schema_version: LAUNCH_TEASER_SCHEMA_VERSION,
            source: LaunchTeaserSourceV1 {
                project_revision: 1,
                projection_digest: "a".repeat(64),
                motion_sha256: "b".repeat(64),
                motion_duration_ms: 60_000,
                motion_width: 1920,
                motion_height: 1080,
            },
            hook: "Test Hook".into(),
            shots: vec![
                shot(1, 0, 5_000),
                shot(2, 5_000, 10_000),
                shot(3, 10_000, 15_000),
            ],
            outro_text: "Made with Rollshot".into(),
            provenance: LaunchTeaserProvenanceV1 {
                deterministic_seed_version: 1,
                agent: None,
                repository_reads: Vec::new(),
                accepted_user_edits: Vec::new(),
            },
        }
    }

    // ---- Repository scope tests ----

    #[test]
    fn scope_add_entry_valid() {
        let mut scope = RepositoryScopeState::new(PathBuf::from("/tmp/test"));
        assert!(scope.add_entry("src/main.rs".into()).is_ok());
        assert!(scope.add_entry("README.md".into()).is_ok());
        assert_eq!(scope.entries.len(), 2);
    }

    #[test]
    fn scope_rejects_parent_dir_traversal() {
        let mut scope = RepositoryScopeState::new(PathBuf::from("/tmp/test"));
        assert_eq!(
            scope.add_entry("../secret.txt".into()),
            Err(ScopeError::InvalidEntry("../secret.txt".into()))
        );
    }

    #[test]
    fn scope_rejects_absolute_entry() {
        let mut scope = RepositoryScopeState::new(PathBuf::from("/tmp/test"));
        assert!(scope.add_entry("/etc/passwd".into()).is_err());
    }

    #[test]
    fn scope_rejects_empty_entry() {
        let mut scope = RepositoryScopeState::new(PathBuf::from("/tmp/test"));
        assert!(scope.add_entry(String::new()).is_err());
    }

    #[test]
    fn scope_deduplicates_entries() {
        let mut scope = RepositoryScopeState::new(PathBuf::from("/tmp/test"));
        scope.add_entry("src/main.rs".into()).unwrap();
        scope.add_entry("src/main.rs".into()).unwrap();
        assert_eq!(scope.entries.len(), 1);
    }

    #[test]
    fn scope_sorts_entries() {
        let mut scope = RepositoryScopeState::new(PathBuf::from("/tmp/test"));
        scope.add_entry("z.txt".into()).unwrap();
        scope.add_entry("a.txt".into()).unwrap();
        assert_eq!(scope.entries, vec!["a.txt", "z.txt"]);
    }

    #[test]
    fn scope_max_entries_limit() {
        let mut scope = RepositoryScopeState::new(PathBuf::from("/tmp/test"));
        for i in 0..MAX_GRANT_ENTRIES {
            scope.add_entry(format!("file{i}.txt")).unwrap();
        }
        assert_eq!(
            scope.add_entry("extra.txt".into()),
            Err(ScopeError::TooManyEntries)
        );
    }

    #[test]
    fn scope_can_confirm_requires_entries() {
        let mut scope = RepositoryScopeState::new(PathBuf::from("/tmp/test"));
        assert!(!scope.can_confirm());
        scope.add_entry("README.md".into()).unwrap();
        assert!(scope.can_confirm());
        scope.confirm();
        assert!(!scope.can_confirm());
    }

    #[test]
    fn scope_remove_entry() {
        let mut scope = RepositoryScopeState::new(PathBuf::from("/tmp/test"));
        scope.add_entry("a.txt".into()).unwrap();
        scope.add_entry("b.txt".into()).unwrap();
        scope.remove_entry(0);
        assert_eq!(scope.entries, vec!["b.txt"]);
    }

    // ---- Patch mapping tests ----

    #[test]
    fn patch_hook_override_produces_diff() {
        let base = valid_plan_fixture();
        let mut patch_plan = base.clone();
        patch_plan.hook = "New Hook".into();

        let review = super::super::launch_teaser::map_agent_patch(&base, &patch_plan).unwrap();
        assert_eq!(review.diffs.len(), 1);
        assert_eq!(
            review.diffs[0].field,
            super::super::launch_teaser::ProposalFieldPath::Hook
        );
        assert_eq!(review.diffs[0].proposed_value, "New Hook");
    }

    #[test]
    fn patch_outro_override_produces_diff() {
        let base = valid_plan_fixture();
        let mut patch_plan = base.clone();
        patch_plan.outro_text = "New Outro".into();

        let review = super::super::launch_teaser::map_agent_patch(&base, &patch_plan).unwrap();
        assert_eq!(review.diffs.len(), 1);
        assert_eq!(
            review.diffs[0].field,
            super::super::launch_teaser::ProposalFieldPath::OutroText
        );
    }

    #[test]
    fn patch_caption_override_produces_diff() {
        let base = valid_plan_fixture();
        let mut patch_plan = base.clone();
        patch_plan.shots[0].caption = "New Caption".into();

        let review = super::super::launch_teaser::map_agent_patch(&base, &patch_plan).unwrap();
        assert!(review
            .diffs
            .iter()
            .any(|d| d.field == super::super::launch_teaser::ProposalFieldPath::ShotCaption(0)));
    }

    #[test]
    fn patch_identical_plans_produce_no_diffs() {
        let base = valid_plan_fixture();
        let review = super::super::launch_teaser::map_agent_patch(&base, &base).unwrap();
        assert!(review.diffs.is_empty());
    }

    #[test]
    fn accepted_candidate_merges_decisions() {
        let base = valid_plan_fixture();
        let mut patch_plan = base.clone();
        patch_plan.hook = "Better Hook".into();
        patch_plan.outro_text = "Better Outro".into();

        let mut review = super::super::launch_teaser::map_agent_patch(&base, &patch_plan).unwrap();

        // Accept hook, reject outro.
        review.diffs[0].decision = FieldDecision::Accepted;
        review.diffs[1].decision = FieldDecision::Rejected;

        let candidate = review.accepted_candidate().unwrap();
        assert_eq!(candidate.hook, "Better Hook");
        assert_eq!(candidate.outro_text, base.outro_text); // rejected, keeps base
    }

    #[test]
    fn all_decided_when_no_pending() {
        let base = valid_plan_fixture();
        let mut patch_plan = base.clone();
        patch_plan.hook = "X".into();

        let mut review = super::super::launch_teaser::map_agent_patch(&base, &patch_plan).unwrap();
        assert!(!review.all_decided());
        review.diffs[0].decision = FieldDecision::Accepted;
        assert!(review.all_decided());
    }

    // ---- Agent state tests ----

    #[test]
    fn agent_state_starts_idle() {
        let state = LaunchTeaserAgentState::Idle;
        assert!(state.is_idle());
    }

    #[test]
    fn agent_state_scope_not_idle() {
        let state = LaunchTeaserAgentState::ScopeSelection(RepositoryScopeState::new(
            PathBuf::from("/tmp"),
        ));
        assert!(!state.is_idle());
    }
}
