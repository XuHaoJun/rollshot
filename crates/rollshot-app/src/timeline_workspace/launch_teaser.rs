//! Launch teaser workspace state, eligibility, and lifecycle.
#![allow(dead_code)]
//!
//! Manages the teaser creation → review → agent → preview → render → complete
//! state machine inside the Timeline Workspace. A teaser can only be created
//! when the project is saved and clean, motion is available, and at least
//! three steps have been reviewed.
//!
//! ```text
//! Closed ──CreateTeaser──► Seeding { operation_id }
//!    ▲                          │ TeaserSeeded(Ok)
//!    │                          ▼
//!    │                    Reviewing { plan, ... }
//!    │                          │
//!    │              ┌───────────┼───────────┐
//!    │              ▼           ▼           ▼
//!    │         AgentRunning  PreviewRendering  FinalRendering
//!    │              │           │           │
//!    │              └───────────┼───────────┘
//!    │                          ▼
//!    └────CloseTeaser────  Completed { ... }
//! ```

use std::path::PathBuf;

use rollshot_action::launch_teaser::{
    LaunchTeaserPlanV1, LaunchTeaserSidecarLoad, ValidatedLaunchTeaserPlan,
};

use super::motion::WorkspaceMotion;
use super::project::{ProjectAccess, ProjectSession};
use super::ProjectSaveState;

/// Reasons why launch teaser creation is disabled.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum LaunchTeaserEligibility {
    /// The project meets all requirements.
    Eligible,
    /// The project has not been saved yet.
    UnsavedProject,
    /// The project is read-only.
    ReadOnlyProject,
    /// The project has unsaved edits.
    DirtyProject,
    /// No motion recording exists.
    MissingMotion,
    /// Motion recording failed or is unavailable.
    UnavailableMotion,
    /// Fewer than 3 reviewed steps.
    TooFewReviewedSteps,
}

impl LaunchTeaserEligibility {
    /// User-visible disabled reason for the Create teaser button.
    pub fn disabled_reason(&self) -> Option<&'static str> {
        match self {
            Self::Eligible => None,
            Self::UnsavedProject => Some("Save this Action Guide before creating a teaser."),
            Self::ReadOnlyProject => Some("This Action Guide is read-only."),
            Self::DirtyProject => Some("Save current guide edits before creating a teaser."),
            Self::MissingMotion => Some("Record motion to create a teaser."),
            Self::UnavailableMotion => Some("The motion recording is unavailable."),
            Self::TooFewReviewedSteps => Some("Review at least 3 steps to create a teaser."),
        }
    }
}

/// Launch teaser workspace state.
#[derive(Debug)]
pub(crate) enum LaunchTeaserState {
    /// No teaser is open.
    Closed,
    /// The deterministic seed is being generated.
    Seeding { operation_id: u64 },
    /// User is reviewing and optionally editing the plan.
    Reviewing(LaunchTeaserReviewState),
    /// Agent run is in flight.
    AgentRunning {
        operation_id: u64,
        review: LaunchTeaserReviewState,
    },
    /// Preview render is in flight.
    PreviewRendering {
        operation_id: u64,
        review: LaunchTeaserReviewState,
        cancellation: rollshot_action::project::PublishCancellation,
    },
    /// Final render is in flight.
    FinalRendering {
        operation_id: u64,
        review: LaunchTeaserReviewState,
        destination: PathBuf,
        cancellation: rollshot_action::project::PublishCancellation,
    },
    /// Teaser has been successfully rendered.
    Completed(LaunchTeaserCompletedState),
}

/// Mutable state for the teaser review screen.
#[derive(Debug, Clone)]
pub(crate) struct LaunchTeaserReviewState {
    /// The current plan (may have user edits).
    pub plan: LaunchTeaserPlanV1,
    /// Last validated plan (None until first validation pass).
    pub validated: Option<ValidatedLaunchTeaserPlan>,
    /// Validation issues from the last edit attempt.
    pub validation_message: Option<String>,
    /// Whether the user has confirmed captured content review.
    pub content_reviewed: bool,
    /// Path to the last successful preview output, if any.
    pub preview_path: Option<PathBuf>,
    /// Monotonic operation ID for the next operation.
    pub next_operation_id: u64,
    /// Whether the plan is stale due to a guide change.
    pub stale: bool,
    /// Stale sidecar loaded from disk, if any.
    pub sidecar: Option<LaunchTeaserSidecarLoad>,
}

impl LaunchTeaserReviewState {
    pub fn new(plan: LaunchTeaserPlanV1) -> Self {
        let validated = plan.validate().ok();
        let validation_message = if validated.is_none() {
            plan.validate().err().map(|e| e.category().to_string())
        } else {
            None
        };
        Self {
            plan,
            validated,
            validation_message,
            content_reviewed: false,
            preview_path: None,
            next_operation_id: 1,
            stale: false,
            sidecar: None,
        }
    }

    /// Re-validate the current plan and update the validated wrapper.
    pub fn revalidate(&mut self) {
        match self.plan.validate() {
            Ok(v) => {
                self.validated = Some(v);
                self.validation_message = None;
            }
            Err(e) => {
                self.validated = None;
                self.validation_message = Some(e.category().to_string());
            }
        }
    }

    /// Whether preview and render should be disabled.
    pub fn render_disabled(&self) -> bool {
        self.stale || self.validated.is_none() || self.validation_message.is_some()
    }

    /// Whether the final render button is gated.
    pub fn final_render_gated(&self) -> bool {
        self.render_disabled() || !self.content_reviewed
    }
}

/// Completed teaser output metadata.
#[derive(Debug, Clone)]
pub(crate) struct LaunchTeaserCompletedState {
    /// Duration in milliseconds.
    pub duration_ms: u64,
    /// Output width.
    pub width: u32,
    /// Output height.
    pub height: u32,
    /// Path to the rendered MP4.
    pub output_path: PathBuf,
    /// SHA-256 of the rendered file.
    pub output_sha256: String,
    /// Whether the sidecar was successfully persisted.
    pub sidecar_persisted: bool,
    /// The accepted plan that was rendered.
    pub plan: LaunchTeaserPlanV1,
}

/// Compute eligibility from the current workspace state.
pub(crate) fn launch_teaser_eligibility(
    save_state: ProjectSaveState,
    project_session: &Option<ProjectSession>,
    motion: &WorkspaceMotion,
    reviewed_step_count: usize,
) -> LaunchTeaserEligibility {
    // Project must be saved
    match project_session {
        None => return LaunchTeaserEligibility::UnsavedProject,
        Some(ProjectSession::Unsaved) => return LaunchTeaserEligibility::UnsavedProject,
        Some(ProjectSession::Saved { access, .. }) => match access {
            ProjectAccess::ReadOnly | ProjectAccess::CorruptReadOnly => {
                return LaunchTeaserEligibility::ReadOnlyProject;
            }
            ProjectAccess::Writable(_) => {}
        },
    }

    // Must be clean
    if save_state != ProjectSaveState::Clean {
        return LaunchTeaserEligibility::DirtyProject;
    }

    // Motion must be available
    match motion {
        WorkspaceMotion::None => return LaunchTeaserEligibility::MissingMotion,
        WorkspaceMotion::Failed(_) | WorkspaceMotion::Unavailable(_) => {
            return LaunchTeaserEligibility::UnavailableMotion;
        }
        WorkspaceMotion::Ready(_) => {}
    }

    // At least 3 reviewed steps
    if reviewed_step_count < rollshot_action::launch_teaser::MIN_SHOTS {
        return LaunchTeaserEligibility::TooFewReviewedSteps;
    }

    LaunchTeaserEligibility::Eligible
}

/// Mark the current teaser review as stale due to a guide change.
pub(crate) fn mark_launch_teaser_stale(state: &mut LaunchTeaserState) {
    if let LaunchTeaserState::Reviewing(review) = state {
        review.stale = true;
        review.preview_path = None;
        review.content_reviewed = false;
    }
}

// ========================================================================
// Task 2: Bounded review edits
// ========================================================================

/// Apply a hook edit to the plan in the review state. Returns true on success.
pub(crate) fn apply_teaser_set_hook(review: &mut LaunchTeaserReviewState, hook: String) -> bool {
    let mut candidate = review.plan.clone();
    candidate.hook = hook;
    match candidate.validate() {
        Ok(_) => {
            review.plan = candidate;
            review.revalidate();
            review.content_reviewed = false;
            review.preview_path = None;
            true
        }
        Err(e) => {
            review.validation_message = Some(e.category().to_string());
            false
        }
    }
}

/// Apply an outro edit.
pub(crate) fn apply_teaser_set_outro(review: &mut LaunchTeaserReviewState, outro: String) -> bool {
    let mut candidate = review.plan.clone();
    candidate.outro_text = outro;
    match candidate.validate() {
        Ok(_) => {
            review.plan = candidate;
            review.revalidate();
            review.content_reviewed = false;
            review.preview_path = None;
            true
        }
        Err(e) => {
            review.validation_message = Some(e.category().to_string());
            false
        }
    }
}

/// Move a shot from one position to another.
pub(crate) fn apply_teaser_move_shot(
    review: &mut LaunchTeaserReviewState,
    from: usize,
    to: usize,
) -> bool {
    if from >= review.plan.shots.len() || to >= review.plan.shots.len() {
        return false;
    }
    let mut candidate = review.plan.clone();
    let shot = candidate.shots.remove(from);
    candidate.shots.insert(to, shot);
    match candidate.validate() {
        Ok(_) => {
            review.plan = candidate;
            review.revalidate();
            review.content_reviewed = false;
            review.preview_path = None;
            true
        }
        Err(e) => {
            review.validation_message = Some(e.category().to_string());
            false
        }
    }
}

/// Set the source range for a specific shot.
pub(crate) fn apply_teaser_set_range(
    review: &mut LaunchTeaserReviewState,
    shot: usize,
    start_ms: u64,
    end_ms: u64,
) -> bool {
    if shot >= review.plan.shots.len() {
        return false;
    }
    let mut candidate = review.plan.clone();
    candidate.shots[shot].source_start_ms = start_ms;
    candidate.shots[shot].source_end_ms = end_ms;
    match candidate.validate() {
        Ok(_) => {
            review.plan = candidate;
            review.revalidate();
            review.content_reviewed = false;
            review.preview_path = None;
            true
        }
        Err(e) => {
            review.validation_message = Some(e.category().to_string());
            false
        }
    }
}

/// Set the focus path for a specific shot.
pub(crate) fn apply_teaser_set_focus(
    review: &mut LaunchTeaserReviewState,
    shot: usize,
    focus: rollshot_action::launch_teaser::FocusPathV1,
) -> bool {
    if shot >= review.plan.shots.len() {
        return false;
    }
    let mut candidate = review.plan.clone();
    candidate.shots[shot].focus_path = focus;
    match candidate.validate() {
        Ok(_) => {
            review.plan = candidate;
            review.revalidate();
            true
        }
        Err(e) => {
            review.validation_message = Some(e.category().to_string());
            false
        }
    }
}

/// Set the speed for a specific shot.
pub(crate) fn apply_teaser_set_speed(
    review: &mut LaunchTeaserReviewState,
    shot: usize,
    speed: rollshot_action::launch_teaser::SpeedV1,
) -> bool {
    if shot >= review.plan.shots.len() {
        return false;
    }
    let mut candidate = review.plan.clone();
    candidate.shots[shot].speed = speed;
    match candidate.validate() {
        Ok(_) => {
            review.plan = candidate;
            review.revalidate();
            review.content_reviewed = false;
            review.preview_path = None;
            true
        }
        Err(e) => {
            review.validation_message = Some(e.category().to_string());
            false
        }
    }
}

/// Set the caption for a specific shot.
pub(crate) fn apply_teaser_set_caption(
    review: &mut LaunchTeaserReviewState,
    shot: usize,
    caption: String,
) -> bool {
    if shot >= review.plan.shots.len() {
        return false;
    }
    let mut candidate = review.plan.clone();
    candidate.shots[shot].caption = caption;
    match candidate.validate() {
        Ok(_) => {
            review.plan = candidate;
            review.revalidate();
            review.content_reviewed = false;
            review.preview_path = None;
            true
        }
        Err(e) => {
            review.validation_message = Some(e.category().to_string());
            false
        }
    }
}

/// Set the transition for a specific shot.
pub(crate) fn apply_teaser_set_transition(
    review: &mut LaunchTeaserReviewState,
    shot: usize,
    transition: rollshot_action::launch_teaser::TransitionV1,
) -> bool {
    if shot >= review.plan.shots.len() {
        return false;
    }
    let mut candidate = review.plan.clone();
    candidate.shots[shot].transition = transition;
    match candidate.validate() {
        Ok(_) => {
            review.plan = candidate;
            review.revalidate();
            review.content_reviewed = false;
            review.preview_path = None;
            true
        }
        Err(e) => {
            review.validation_message = Some(e.category().to_string());
            false
        }
    }
}

// ========================================================================
// Task 2: Agent proposal review
// ========================================================================

/// Field paths that can differ between base plan and agent proposal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ProposalFieldPath {
    Hook,
    OutroText,
    ShotCaption(usize),
    ShotSpeed(usize),
    ShotTransition(usize),
    ShotFocus(usize),
    ShotRange(usize),
}

/// Decision state for a single proposed field.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum FieldDecision {
    Pending,
    Accepted,
    Rejected,
}

/// A single field-level proposal diff.
#[derive(Debug, Clone)]
pub(crate) struct ProposalDiffEntry {
    pub field: ProposalFieldPath,
    pub current_value: String,
    pub proposed_value: String,
    pub decision: FieldDecision,
}

/// Agent proposal review with per-field decisions.
#[derive(Debug, Clone)]
pub(crate) struct LaunchTeaserAgentProposalReview {
    pub base_plan: rollshot_action::launch_teaser::LaunchTeaserPlanV1,
    pub proposed_plan: rollshot_action::launch_teaser::LaunchTeaserPlanV1,
    pub diffs: Vec<ProposalDiffEntry>,
}

impl LaunchTeaserAgentProposalReview {
    pub fn current_plan(&self) -> &rollshot_action::launch_teaser::LaunchTeaserPlanV1 {
        &self.base_plan
    }

    pub fn proposed_plan(&self) -> &rollshot_action::launch_teaser::LaunchTeaserPlanV1 {
        &self.proposed_plan
    }

    /// Whether all fields have been decided.
    pub fn all_decided(&self) -> bool {
        self.diffs
            .iter()
            .all(|d| d.decision != FieldDecision::Pending)
    }

    /// Whether all fields are accepted.
    pub fn all_accepted(&self) -> bool {
        self.diffs
            .iter()
            .all(|d| d.decision == FieldDecision::Accepted)
    }

    /// Build a candidate plan from currently accepted decisions.
    pub fn accepted_candidate(&self) -> Option<rollshot_action::launch_teaser::LaunchTeaserPlanV1> {
        let mut candidate = self.base_plan.clone();
        for diff in &self.diffs {
            if diff.decision == FieldDecision::Accepted {
                apply_diff_to_plan(&mut candidate, &diff.field, &diff.proposed_value);
            }
        }
        Some(candidate)
    }
}

/// Map an agent patch onto a base plan, producing a review with per-field decisions.
pub(crate) fn map_agent_patch(
    base: &rollshot_action::launch_teaser::LaunchTeaserPlanV1,
    patch: &rollshot_action::launch_teaser::LaunchTeaserPlanV1,
) -> Result<LaunchTeaserAgentProposalReview, String> {
    // Validate the proposed plan first
    patch.validate().map_err(|e| e.category().to_string())?;

    let mut diffs = Vec::new();

    // Compare hook
    if base.hook != patch.hook {
        diffs.push(ProposalDiffEntry {
            field: ProposalFieldPath::Hook,
            current_value: base.hook.clone(),
            proposed_value: patch.hook.clone(),
            decision: FieldDecision::Pending,
        });
    }

    // Compare outro
    if base.outro_text != patch.outro_text {
        diffs.push(ProposalDiffEntry {
            field: ProposalFieldPath::OutroText,
            current_value: base.outro_text.clone(),
            proposed_value: patch.outro_text.clone(),
            decision: FieldDecision::Pending,
        });
    }

    // Compare shots
    let max_shots = base.shots.len().max(patch.shots.len());
    for i in 0..max_shots {
        match (base.shots.get(i), patch.shots.get(i)) {
            (Some(base_shot), Some(patch_shot)) => {
                if base_shot.caption != patch_shot.caption {
                    diffs.push(ProposalDiffEntry {
                        field: ProposalFieldPath::ShotCaption(i),
                        current_value: base_shot.caption.clone(),
                        proposed_value: patch_shot.caption.clone(),
                        decision: FieldDecision::Pending,
                    });
                }
                if base_shot.speed != patch_shot.speed {
                    diffs.push(ProposalDiffEntry {
                        field: ProposalFieldPath::ShotSpeed(i),
                        current_value: format!("{:?}", base_shot.speed),
                        proposed_value: format!("{:?}", patch_shot.speed),
                        decision: FieldDecision::Pending,
                    });
                }
                if base_shot.transition != patch_shot.transition {
                    diffs.push(ProposalDiffEntry {
                        field: ProposalFieldPath::ShotTransition(i),
                        current_value: format!("{:?}", base_shot.transition),
                        proposed_value: format!("{:?}", patch_shot.transition),
                        decision: FieldDecision::Pending,
                    });
                }
                if base_shot.focus_path != patch_shot.focus_path {
                    diffs.push(ProposalDiffEntry {
                        field: ProposalFieldPath::ShotFocus(i),
                        current_value: format!("{:?}", base_shot.focus_path),
                        proposed_value: format!("{:?}", patch_shot.focus_path),
                        decision: FieldDecision::Pending,
                    });
                }
                if base_shot.source_start_ms != patch_shot.source_start_ms
                    || base_shot.source_end_ms != patch_shot.source_end_ms
                {
                    diffs.push(ProposalDiffEntry {
                        field: ProposalFieldPath::ShotRange(i),
                        current_value: format!(
                            "{}-{}",
                            base_shot.source_start_ms, base_shot.source_end_ms
                        ),
                        proposed_value: format!(
                            "{}-{}",
                            patch_shot.source_start_ms, patch_shot.source_end_ms
                        ),
                        decision: FieldDecision::Pending,
                    });
                }
            }
            _ => {
                // Shot count changed
                diffs.push(ProposalDiffEntry {
                    field: ProposalFieldPath::ShotCaption(i),
                    current_value: String::new(),
                    proposed_value: "shot added/removed".into(),
                    decision: FieldDecision::Pending,
                });
            }
        }
    }

    Ok(LaunchTeaserAgentProposalReview {
        base_plan: base.clone(),
        proposed_plan: patch.clone(),
        diffs,
    })
}

/// Apply a single diff field value to a plan (for building accepted candidates).
fn apply_diff_to_plan(
    plan: &mut rollshot_action::launch_teaser::LaunchTeaserPlanV1,
    field: &ProposalFieldPath,
    value: &str,
) {
    match field {
        ProposalFieldPath::Hook => plan.hook = value.to_string(),
        ProposalFieldPath::OutroText => plan.outro_text = value.to_string(),
        ProposalFieldPath::ShotCaption(i) => {
            if let Some(shot) = plan.shots.get_mut(*i) {
                shot.caption = value.to_string();
            }
        }
        _ => {} // Speed, transition, focus, range require typed parsing; skip for now
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn eligibility_unsaved_project() {
        assert_eq!(
            launch_teaser_eligibility(ProjectSaveState::Unsaved, &None, &WorkspaceMotion::None, 5,),
            LaunchTeaserEligibility::UnsavedProject
        );
    }

    #[test]
    fn eligibility_dirty_project() {
        use crate::timeline_workspace::project::ProjectAccess;
        use std::path::PathBuf;

        let session = Some(ProjectSession::Saved {
            root: PathBuf::from("/tmp/test"),
            base_revision: 1,
            access: ProjectAccess::Writable(
                crate::timeline_workspace::project::ProjectWriterGuard::for_test(),
            ),
        });
        assert_eq!(
            launch_teaser_eligibility(ProjectSaveState::Dirty, &session, &WorkspaceMotion::None, 5,),
            LaunchTeaserEligibility::DirtyProject
        );
    }

    #[test]
    fn eligibility_missing_motion() {
        use crate::timeline_workspace::project::ProjectAccess;
        use std::path::PathBuf;

        let session = Some(ProjectSession::Saved {
            root: PathBuf::from("/tmp/test"),
            base_revision: 1,
            access: ProjectAccess::Writable(
                crate::timeline_workspace::project::ProjectWriterGuard::for_test(),
            ),
        });
        assert_eq!(
            launch_teaser_eligibility(ProjectSaveState::Clean, &session, &WorkspaceMotion::None, 5,),
            LaunchTeaserEligibility::MissingMotion
        );
    }

    #[test]
    fn eligibility_too_few_steps() {
        use crate::timeline_workspace::project::ProjectAccess;
        use std::path::PathBuf;

        let session = Some(ProjectSession::Saved {
            root: PathBuf::from("/tmp/test"),
            base_revision: 1,
            access: ProjectAccess::Writable(
                crate::timeline_workspace::project::ProjectWriterGuard::for_test(),
            ),
        });
        // Motion must be ready for this test since motion is checked before step count
        let ready_motion =
            WorkspaceMotion::Ready(rollshot_action::ValidatedMotionAsset::new_for_test(
                rollshot_action::motion::MotionMetadata {
                    sha256: "c".repeat(64),
                    duration_ms: 60_000,
                    width: 1920,
                    height: 1080,
                    fps_numerator: 30,
                    fps_denominator: 1,
                    codec: rollshot_action::motion::MotionCodec::H264,
                    audio: rollshot_action::motion::MotionAudio::None,
                },
                std::path::PathBuf::from("/tmp/test/motion.mp4"),
                std::path::PathBuf::from("/tmp/test/scratch"),
            ));
        assert_eq!(
            launch_teaser_eligibility(ProjectSaveState::Clean, &session, &ready_motion, 2,),
            LaunchTeaserEligibility::TooFewReviewedSteps
        );
    }

    #[test]
    fn eligibility_read_only() {
        use crate::timeline_workspace::project::ProjectAccess;
        use std::path::PathBuf;

        let session = Some(ProjectSession::Saved {
            root: PathBuf::from("/tmp/test"),
            base_revision: 1,
            access: ProjectAccess::ReadOnly,
        });
        assert_eq!(
            launch_teaser_eligibility(ProjectSaveState::Clean, &session, &WorkspaceMotion::None, 5,),
            LaunchTeaserEligibility::ReadOnlyProject
        );
    }

    #[test]
    fn disabled_reasons_are_exact() {
        assert_eq!(
            LaunchTeaserEligibility::UnsavedProject.disabled_reason(),
            Some("Save this Action Guide before creating a teaser.")
        );
        assert_eq!(
            LaunchTeaserEligibility::ReadOnlyProject.disabled_reason(),
            Some("This Action Guide is read-only.")
        );
        assert_eq!(
            LaunchTeaserEligibility::DirtyProject.disabled_reason(),
            Some("Save current guide edits before creating a teaser.")
        );
        assert_eq!(
            LaunchTeaserEligibility::MissingMotion.disabled_reason(),
            Some("Record motion to create a teaser.")
        );
        assert_eq!(
            LaunchTeaserEligibility::UnavailableMotion.disabled_reason(),
            Some("The motion recording is unavailable.")
        );
        assert_eq!(
            LaunchTeaserEligibility::TooFewReviewedSteps.disabled_reason(),
            Some("Review at least 3 steps to create a teaser.")
        );
        assert_eq!(LaunchTeaserEligibility::Eligible.disabled_reason(), None);
    }

    fn valid_plan_fixture() -> rollshot_action::launch_teaser::LaunchTeaserPlanV1 {
        use rollshot_action::launch_teaser::*;
        use rollshot_action::project::ProjectStepId;

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
                caption: String::new(),
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
            outro_text: "Test Outro".into(),
            provenance: LaunchTeaserProvenanceV1 {
                deterministic_seed_version: 1,
                agent: None,
                repository_reads: Vec::new(),
                accepted_user_edits: Vec::new(),
            },
        }
    }

    #[test]
    fn review_state_detects_stale() {
        let plan = valid_plan_fixture();
        let mut review = LaunchTeaserReviewState::new(plan);
        assert!(!review.render_disabled());
        review.stale = true;
        assert!(review.render_disabled());
    }

    #[test]
    fn review_state_gates_final_render() {
        let plan = valid_plan_fixture();
        let mut review = LaunchTeaserReviewState::new(plan);
        assert!(review.final_render_gated());
        review.content_reviewed = true;
        assert!(!review.final_render_gated());
    }
}

// ========================================================================
// Task 6: Completion tests
// ========================================================================

#[cfg(test)]
pub(crate) mod completion_tests {
    use rollshot_action::launch_teaser::*;
    use rollshot_action::project::ProjectStepId;
    use rollshot_action::{
        CandidateKind, CaptureRegion, DegradedReason, DetectReason, FrameId, InputCapability,
        InputSourceKind, Millis,
    };

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

    #[test]
    fn sidecar_written_after_verified_render() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();

        let loaded = build_test_loaded_project(root);
        let plan = seed_launch_teaser(&loaded).unwrap();
        let plan_sha256 = persistence::compute_plan_sha256(&plan).unwrap();
        let output_path = root.join("output.mp4");
        std::fs::write(&output_path, b"fake video data").unwrap();

        // Compute output SHA-256.
        let output_bytes = std::fs::read(&output_path).unwrap();
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(&output_bytes);
        let output_sha256 = format!("{:x}", hasher.finalize());

        let now_ms = 1_700_000_000_000i64;
        let artifact = error::LaunchTeaserArtifactV1 {
            schema_version: 1,
            plan: plan.clone(),
            plan_sha256,
            renderer_version: 1,
            ffmpeg_version: "6.0".into(),
            ffprobe_version: "6.0".into(),
            output_sha256: output_sha256.clone(),
            rendered_at_unix_ms: now_ms,
        };

        let result = persistence::write_launch_teaser_sidecar(root, &artifact);
        assert!(result.is_ok(), "sidecar write should succeed: {result:?}");

        // Verify the sidecar file exists.
        let sidecar_path = root.join(persistence::SIDECAR_RELATIVE_PATH);
        assert!(sidecar_path.exists(), "sidecar file should exist");

        // Verify it can be loaded.
        let load_result = persistence::load_launch_teaser_sidecar(root, &loaded);
        match load_result {
            error::LaunchTeaserSidecarLoad::Available(loaded_artifact) => {
                assert_eq!(loaded_artifact.output_sha256, output_sha256);
                assert_eq!(loaded_artifact.plan.hook, plan.hook);
            }
            other => panic!("expected Available, got {other:?}"),
        }
    }

    #[test]
    fn sidecar_failure_preserves_external_mp4() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();

        let loaded = build_test_loaded_project(root);
        let plan = seed_launch_teaser(&loaded).unwrap();
        let plan_sha256 = persistence::compute_plan_sha256(&plan).unwrap();
        let output_path = root.join("output.mp4");
        std::fs::write(&output_path, b"fake video data").unwrap();

        // Write a valid sidecar first.
        let artifact = error::LaunchTeaserArtifactV1 {
            schema_version: 1,
            plan: plan.clone(),
            plan_sha256: plan_sha256.clone(),
            renderer_version: 1,
            ffmpeg_version: "6.0".into(),
            ffprobe_version: "6.0".into(),
            output_sha256: "c".repeat(64),
            rendered_at_unix_ms: 1_700_000_000_000,
        };
        persistence::write_launch_teaser_sidecar(root, &artifact).unwrap();

        // Now try to write a sidecar with a bad digest.
        let bad_artifact = error::LaunchTeaserArtifactV1 {
            schema_version: 1,
            plan: plan.clone(),
            plan_sha256: "bad".repeat(32), // wrong digest
            renderer_version: 1,
            ffmpeg_version: "6.0".into(),
            ffprobe_version: "6.0".into(),
            output_sha256: "c".repeat(64),
            rendered_at_unix_ms: 1_700_000_000_001,
        };
        let result = persistence::write_launch_teaser_sidecar(root, &bad_artifact);
        assert!(result.is_err(), "should fail with bad digest");

        // External MP4 must still exist.
        assert!(
            output_path.exists(),
            "external MP4 must survive sidecar failure"
        );

        // Original sidecar must still be valid.
        let load_result = persistence::load_launch_teaser_sidecar(root, &loaded);
        assert!(
            matches!(load_result, error::LaunchTeaserSidecarLoad::Available(_)),
            "original sidecar should still be loadable"
        );
    }

    #[test]
    fn sidecar_does_not_increment_project_revision() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();

        let loaded = build_test_loaded_project(root);
        let plan = seed_launch_teaser(&loaded).unwrap();
        let plan_sha256 = persistence::compute_plan_sha256(&plan).unwrap();

        let artifact = error::LaunchTeaserArtifactV1 {
            schema_version: 1,
            plan: plan.clone(),
            plan_sha256,
            renderer_version: 1,
            ffmpeg_version: "6.0".into(),
            ffprobe_version: "6.0".into(),
            output_sha256: "c".repeat(64),
            rendered_at_unix_ms: 1_700_000_000_000,
        };

        let _ = persistence::write_launch_teaser_sidecar(root, &artifact);

        // The sidecar is in publish/, not at the manifest level.
        let sidecar_path = root.join(persistence::SIDECAR_RELATIVE_PATH);
        assert!(sidecar_path.exists());
        // The sidecar path is `publish/launch-teaser-plan-v1.json`,
        // separate from the project manifest.
        assert!(
            !root.join("manifest-v3.json").exists(),
            "sidecar must not touch the project manifest"
        );
    }

    #[test]
    fn output_digest_recorded_in_sidecar() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();

        let loaded = build_test_loaded_project(root);
        let plan = seed_launch_teaser(&loaded).unwrap();
        let plan_sha256 = persistence::compute_plan_sha256(&plan).unwrap();

        let output_data = b"unique video content for digest test";
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(output_data);
        let expected_digest = format!("{:x}", hasher.finalize());

        let artifact = error::LaunchTeaserArtifactV1 {
            schema_version: 1,
            plan: plan.clone(),
            plan_sha256,
            renderer_version: 1,
            ffmpeg_version: "6.0".into(),
            ffprobe_version: "6.0".into(),
            output_sha256: expected_digest.clone(),
            rendered_at_unix_ms: 1_700_000_000_000,
        };

        persistence::write_launch_teaser_sidecar(root, &artifact).unwrap();

        let load_result = persistence::load_launch_teaser_sidecar(root, &loaded);
        match load_result {
            error::LaunchTeaserSidecarLoad::Available(a) => {
                assert_eq!(a.output_sha256, expected_digest);
            }
            other => panic!("expected Available, got {other:?}"),
        }
    }

    #[test]
    fn guide_change_marks_sidecar_stale() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();

        let loaded = build_test_loaded_project(root);
        let plan = seed_launch_teaser(&loaded).unwrap();
        let plan_sha256 = persistence::compute_plan_sha256(&plan).unwrap();

        let artifact = error::LaunchTeaserArtifactV1 {
            schema_version: 1,
            plan: plan.clone(),
            plan_sha256,
            renderer_version: 1,
            ffmpeg_version: "6.0".into(),
            ffprobe_version: "6.0".into(),
            output_sha256: "c".repeat(64),
            rendered_at_unix_ms: 1_700_000_000_000,
        };

        persistence::write_launch_teaser_sidecar(root, &artifact).unwrap();

        // With matching steps, the sidecar should be fresh.
        let load_result = persistence::load_launch_teaser_sidecar(root, &loaded);
        assert!(
            matches!(load_result, error::LaunchTeaserSidecarLoad::Available(_)),
            "sidecar should be fresh with matching steps"
        );

        // Build a loaded project with different steps to simulate a guide change.
        let mut stale_manifest = loaded.manifest.clone();
        stale_manifest.steps.remove(2); // Remove step 3
        let stale_loaded = rollshot_action::project::LoadedProject {
            root: loaded.root.clone(),
            manifest: stale_manifest,
            motion: loaded.motion.clone(),
        };
        let load_result = persistence::load_launch_teaser_sidecar(root, &stale_loaded);
        assert!(
            matches!(load_result, error::LaunchTeaserSidecarLoad::Stale(_)),
            "sidecar should be stale after guide change"
        );
    }

    #[test]
    fn no_mp4_duplicate_in_project() {
        // The sidecar stores only the plan and metadata, not the MP4 data.
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();

        let plan = valid_plan_fixture();
        let plan_sha256 = persistence::compute_plan_sha256(&plan).unwrap();

        let artifact = error::LaunchTeaserArtifactV1 {
            schema_version: 1,
            plan: plan.clone(),
            plan_sha256,
            renderer_version: 1,
            ffmpeg_version: "6.0".into(),
            ffprobe_version: "6.0".into(),
            output_sha256: "c".repeat(64),
            rendered_at_unix_ms: 1_700_000_000_000,
        };

        persistence::write_launch_teaser_sidecar(root, &artifact).unwrap();

        // Read the sidecar file.
        let sidecar_path = root.join(persistence::SIDECAR_RELATIVE_PATH);
        let sidecar_bytes = std::fs::read(&sidecar_path).unwrap();

        // The sidecar is JSON, not binary MP4 data.
        let sidecar_str = String::from_utf8(sidecar_bytes.clone()).unwrap();
        assert!(
            sidecar_str.contains("\"plan\""),
            "sidecar should contain plan"
        );
        assert!(
            !sidecar_bytes.windows(4).any(|w| w == b"\x00\x00\x00\x01"),
            "sidecar must not contain MP4 NAL units"
        );
    }

    /// Helper: build a minimal LoadedProject for sidecar freshness checks.
    fn build_test_loaded_project(
        root: &std::path::Path,
    ) -> rollshot_action::project::LoadedProject {
        use rollshot_action::motion::asset::ValidatedMotionAsset;
        use rollshot_action::motion::probe::{MotionAudio, MotionCodec, MotionMetadata};
        use rollshot_action::project::*;

        let steps: Vec<ProjectStep> = (1..=3)
            .map(|i| ProjectStep {
                id: ProjectStepId(i),
                order: i as u32,
                title: format!("Step {i}"),
                caption: Some(format!("Caption {i}")),
                kind: CandidateKind::Click,
                reason: DetectReason::VisualChange,
                at_ms: i * 3_000,
                keyframe: i as FrameId,
                nearby: vec![i as FrameId],
                annotations: None,
            })
            .collect();

        let frames: Vec<ProjectFrame> = (1..=3)
            .map(|i| ProjectFrame {
                id: i as FrameId,
                at_ms: (i as u64 * 3_000) as Millis,
                sha256: "b".repeat(64),
                width: 1920,
                height: 1080,
            })
            .collect();

        let manifest = ProjectManifestV3 {
            schema_version: 3,
            revision: 1,
            title: "Test Guide".into(),
            capture_region: CaptureRegion {
                x: 0,
                y: 0,
                width: 1920,
                height: 1080,
            },
            input_source: InputSourceKind::VisualOnly,
            input_capability: InputCapability::VisualOnly {
                reason: DegradedReason::SourceStartFailed,
            },
            enabled_outputs: EnabledOutputs::default(),
            frames,
            steps,
            import_warnings: Vec::new(),
            motion: Some(MotionAsset {
                relative_path: MotionAsset::CANONICAL_PATH.into(),
                sha256: "a".repeat(64),
                duration_ms: 30_000,
                width: 1920,
                height: 1080,
                fps_numerator: 30,
                fps_denominator: 1,
                codec: "h264".into(),
                audio: "none".into(),
            }),
        };

        let motion_meta = MotionMetadata {
            sha256: "a".repeat(64),
            duration_ms: 30_000,
            width: 1920,
            height: 1080,
            fps_numerator: 30,
            fps_denominator: 1,
            codec: MotionCodec::H264,
            audio: MotionAudio::None,
        };

        let scratch = tempfile::tempdir().unwrap();
        let mp4 = scratch.path().join("recording.mp4");
        std::fs::write(&mp4, b"fake mp4").unwrap();
        let motion = MotionAssetLoad::Available(ValidatedMotionAsset::new_for_test(
            motion_meta,
            mp4,
            scratch.path().to_path_buf(),
        ));

        LoadedProject {
            root: root.to_path_buf(),
            manifest,
            motion,
        }
    }
}
