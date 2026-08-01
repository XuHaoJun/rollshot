//! Deterministic seed generation and binding validation.
//!
//! The seed algorithm selects reviewed steps, allocates non-overlapping
//! source windows, and produces a default plan with fixed focus, speed,
//! and transition settings. All choices are deterministic from the project
//! state.

use crate::project::continuity::ActionGuideContextProjectionV1;
use crate::project::{LoadedProject, MotionAssetLoad, ProjectStepId};

use super::error::{LaunchTeaserBindingError, LaunchTeaserSeedError};
use super::plan::{
    FocusPathV1, LaunchTeaserPlanV1, LaunchTeaserProvenanceV1, LaunchTeaserShotV1,
    LaunchTeaserSourceV1, NormalizedPointV1, SpeedV1, TransitionV1, LAUNCH_TEASER_SCHEMA_VERSION,
    MAX_SHOTS,
};

/// Deterministic seed algorithm version.
pub const DETERMINISTIC_SEED_VERSION: u32 = 1;

const DEFAULT_FOCUS: FocusPathV1 = FocusPathV1 {
    start: NormalizedPointV1 { x: 5_000, y: 5_000 },
    end: NormalizedPointV1 { x: 5_000, y: 5_000 },
    zoom_permille: 1_000,
};

/// Target displayed durations per shot count (milliseconds).
fn target_durations(shot_count: usize) -> Vec<u64> {
    match shot_count {
        3 => vec![5_000, 5_000, 5_000],
        4 => vec![4_000, 4_000, 4_000, 4_000],
        5 => vec![3_500, 3_500, 3_500, 3_500, 3_500],
        _ => unreachable!(),
    }
}

/// Select evenly-spaced indices from `step_count` steps, keeping first and last.
fn selected_indices(step_count: usize) -> Vec<usize> {
    let wanted = step_count.min(MAX_SHOTS);
    if wanted == step_count {
        return (0..step_count).collect();
    }
    (0..wanted)
        .map(|slot| slot * (step_count - 1) / (wanted - 1))
        .collect()
}

/// Generate a deterministic launch teaser plan from a loaded project.
pub fn seed_launch_teaser(
    loaded: &LoadedProject,
) -> Result<LaunchTeaserPlanV1, LaunchTeaserSeedError> {
    let manifest = &loaded.manifest;

    // Need at least MIN_SHOTS reviewed steps.
    if manifest.steps.len() < super::plan::MIN_SHOTS {
        return Err(LaunchTeaserSeedError::InsufficientSteps);
    }

    // Need an available motion asset.
    let motion = match &loaded.motion {
        MotionAssetLoad::Available(m) => m,
        _ => return Err(LaunchTeaserSeedError::InsufficientMotion),
    };

    let motion_duration_ms = motion.duration_ms();
    let motion_width = motion.width();
    let motion_height = motion.height();
    let motion_sha256 = motion.sha256().to_string();

    // Build projection for digest.
    let projection = ActionGuideContextProjectionV1::from_loaded_project(loaded)
        .map_err(|_| LaunchTeaserSeedError::InsufficientSteps)?;

    // Select steps.
    let indices = selected_indices(manifest.steps.len());
    let shot_count = indices.len();
    let durations = target_durations(shot_count);

    // Allocate source windows around each step's at_ms.
    let windows = allocate_windows(&indices, &durations, &manifest.steps, motion_duration_ms)?;

    // Build shots.
    let shots: Vec<LaunchTeaserShotV1> = indices
        .iter()
        .zip(windows.iter())
        .zip(durations.iter())
        .map(|((&idx, &(start, end)), _dur)| {
            let step = &manifest.steps[idx];
            let caption = step.caption.clone().unwrap_or_else(|| step.title.clone());
            LaunchTeaserShotV1 {
                reviewed_step_id: step.id,
                source_start_ms: start,
                source_end_ms: end,
                focus_path: DEFAULT_FOCUS,
                speed: SpeedV1::P1000,
                caption,
                transition: TransitionV1::Cut,
            }
        })
        .collect();

    // Build provenance.
    let provenance = LaunchTeaserProvenanceV1 {
        deterministic_seed_version: DETERMINISTIC_SEED_VERSION,
        agent: None,
        repository_reads: Vec::new(),
        accepted_user_edits: Vec::new(),
    };

    // Build source binding.
    let source = LaunchTeaserSourceV1 {
        project_revision: manifest.revision,
        projection_digest: projection.digest().to_string(),
        motion_sha256,
        motion_duration_ms,
        motion_width,
        motion_height,
    };

    Ok(LaunchTeaserPlanV1 {
        schema_version: LAUNCH_TEASER_SCHEMA_VERSION,
        source,
        hook: manifest.title.clone(),
        shots,
        outro_text: "Made with Rollshot".into(),
        provenance,
    })
}

/// Allocate non-overlapping source windows centered on each step's `at_ms`.
///
/// Each window is `[at_ms - half_duration, at_ms + half_duration]`, clamped
/// to `[0, motion_duration_ms]`. Adjacent windows are shifted without
/// reordering to eliminate overlap.
fn allocate_windows(
    indices: &[usize],
    durations: &[u64],
    steps: &[crate::project::ProjectStep],
    motion_duration_ms: u64,
) -> Result<Vec<(u64, u64)>, LaunchTeaserSeedError> {
    let mut windows: Vec<(u64, u64)> = Vec::with_capacity(indices.len());

    for (&idx, &dur) in indices.iter().zip(durations.iter()) {
        let at_ms = steps[idx].at_ms;
        let half = dur / 2;
        let mut start = at_ms.saturating_sub(half);
        let mut end = start + dur;

        // Clamp to motion bounds.
        if end > motion_duration_ms {
            end = motion_duration_ms;
            start = end.saturating_sub(dur);
        }
        if start > motion_duration_ms {
            return Err(LaunchTeaserSeedError::InsufficientMotion);
        }

        // Shift to avoid overlap with previous window.
        if let Some(&(_, prev_end)) = windows.last() {
            if start < prev_end {
                let shift = prev_end - start;
                start = prev_end;
                end = end.saturating_add(shift);
                if end > motion_duration_ms {
                    // Try to fit by reducing start.
                    end = motion_duration_ms;
                    start = end.saturating_sub(dur);
                    if start < prev_end {
                        return Err(LaunchTeaserSeedError::InsufficientMotion);
                    }
                }
            }
        }

        windows.push((start, end));
    }

    Ok(windows)
}

/// Validate that a plan still binds to the current project state.
pub fn validate_launch_teaser_binding(
    plan: &LaunchTeaserPlanV1,
    loaded: &LoadedProject,
) -> Result<(), LaunchTeaserBindingError> {
    let manifest = &loaded.manifest;

    // Check project revision.
    if plan.source.project_revision != manifest.revision {
        return Err(LaunchTeaserBindingError::StaleProject);
    }

    // Rebuild projection and compare digest.
    let projection = ActionGuideContextProjectionV1::from_loaded_project(loaded)
        .map_err(|_| LaunchTeaserBindingError::StaleProject)?;
    if plan.source.projection_digest != projection.digest() {
        return Err(LaunchTeaserBindingError::StaleProject);
    }

    // Check motion asset.
    let motion = match &loaded.motion {
        MotionAssetLoad::Available(m) => m,
        _ => return Err(LaunchTeaserBindingError::StaleMotion),
    };
    if plan.source.motion_sha256 != motion.sha256()
        || plan.source.motion_duration_ms != motion.duration_ms()
        || plan.source.motion_width != motion.width()
        || plan.source.motion_height != motion.height()
    {
        return Err(LaunchTeaserBindingError::StaleMotion);
    }

    // Check every referenced step ID exists.
    let step_ids: std::collections::HashSet<ProjectStepId> =
        manifest.steps.iter().map(|s| s.id).collect();
    for shot in &plan.shots {
        if !step_ids.contains(&shot.reviewed_step_id) {
            return Err(LaunchTeaserBindingError::MissingStep);
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{
        CandidateKind, CaptureRegion, DetectReason, FrameId, InputCapability, InputSourceKind,
        Millis,
    };
    use crate::motion::asset::ValidatedMotionAsset;
    use crate::motion::probe::{MotionAudio, MotionCodec, MotionMetadata};
    use crate::project::{
        EnabledOutputs, MotionAsset, ProjectFrame, ProjectManifestV3, ProjectStep,
    };
    use std::path::PathBuf;

    fn test_motion_metadata() -> MotionMetadata {
        MotionMetadata {
            sha256: "a".repeat(64),
            duration_ms: 30_000,
            width: 1920,
            height: 1080,
            fps_numerator: 30,
            fps_denominator: 1,
            codec: MotionCodec::H264,
            audio: MotionAudio::None,
        }
    }

    fn loaded_project_with_steps(step_count: usize) -> LoadedProject {
        let steps: Vec<ProjectStep> = (1..=step_count)
            .map(|i| ProjectStep {
                id: ProjectStepId(i as u64),
                order: i as u32,
                title: format!("Step {i}"),
                caption: Some(format!("Caption {i}")),
                kind: CandidateKind::Click,
                reason: DetectReason::VisualChange,
                at_ms: ((i as u64) * 3_000) as Millis,
                keyframe: i as FrameId,
                nearby: vec![i as FrameId],
                annotations: None,
            })
            .collect();

        let frames: Vec<ProjectFrame> = (1..=step_count)
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
                reason: crate::models::DegradedReason::SourceStartFailed,
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

        let scratch = tempfile::tempdir().unwrap();
        let mp4 = scratch.path().join("recording.mp4");
        std::fs::write(&mp4, b"fake mp4").unwrap();
        let motion = MotionAssetLoad::Available(ValidatedMotionAsset::new_for_test(
            test_motion_metadata(),
            mp4,
            scratch.path().to_path_buf(),
        ));

        LoadedProject {
            root: PathBuf::from("/tmp/test-project"),
            manifest,
            motion,
        }
    }

    #[test]
    fn seed_keeps_first_last_and_evenly_samples_interior() {
        let loaded = loaded_project_with_steps(8);
        let plan = seed_launch_teaser(&loaded).unwrap();
        let ids: Vec<u64> = plan
            .shots
            .iter()
            .map(|shot| shot.reviewed_step_id.0)
            .collect();
        assert_eq!(ids, vec![1, 2, 4, 6, 8]);
    }

    #[test]
    fn seed_three_steps_all_selected() {
        let loaded = loaded_project_with_steps(3);
        let plan = seed_launch_teaser(&loaded).unwrap();
        assert_eq!(plan.shots.len(), 3);
        let ids: Vec<u64> = plan.shots.iter().map(|s| s.reviewed_step_id.0).collect();
        assert_eq!(ids, vec![1, 2, 3]);
    }

    #[test]
    fn seed_five_steps_all_selected() {
        let loaded = loaded_project_with_steps(5);
        let plan = seed_launch_teaser(&loaded).unwrap();
        assert_eq!(plan.shots.len(), 5);
        let ids: Vec<u64> = plan.shots.iter().map(|s| s.reviewed_step_id.0).collect();
        assert_eq!(ids, vec![1, 2, 3, 4, 5]);
    }

    #[test]
    fn seed_rejects_two_steps() {
        let loaded = loaded_project_with_steps(2);
        assert_eq!(
            seed_launch_teaser(&loaded).unwrap_err().category(),
            "insufficient-steps"
        );
    }

    #[test]
    fn seed_rejects_zero_steps() {
        let mut loaded = loaded_project_with_steps(3);
        loaded.manifest.steps.clear();
        assert_eq!(
            seed_launch_teaser(&loaded).unwrap_err().category(),
            "insufficient-steps"
        );
    }

    #[test]
    fn seed_rejects_unavailable_motion() {
        let mut loaded = loaded_project_with_steps(3);
        loaded.motion = MotionAssetLoad::None;
        assert_eq!(
            seed_launch_teaser(&loaded).unwrap_err().category(),
            "insufficient-motion"
        );
    }

    #[test]
    fn seed_uses_project_title_as_hook() {
        let loaded = loaded_project_with_steps(3);
        let plan = seed_launch_teaser(&loaded).unwrap();
        assert_eq!(plan.hook, "Test Guide");
    }

    #[test]
    fn seed_uses_caption_or_title() {
        let loaded = loaded_project_with_steps(3);
        let plan = seed_launch_teaser(&loaded).unwrap();
        assert_eq!(plan.shots[0].caption, "Caption 1");
    }

    #[test]
    fn seed_falls_back_to_title_when_no_caption() {
        let mut loaded = loaded_project_with_steps(3);
        loaded.manifest.steps[0].caption = None;
        let plan = seed_launch_teaser(&loaded).unwrap();
        assert_eq!(plan.shots[0].caption, "Step 1");
    }

    #[test]
    fn seed_uses_default_focus_and_speed() {
        let loaded = loaded_project_with_steps(3);
        let plan = seed_launch_teaser(&loaded).unwrap();
        for shot in &plan.shots {
            assert_eq!(shot.focus_path, DEFAULT_FOCUS);
            assert_eq!(shot.speed, SpeedV1::P1000);
            assert_eq!(shot.transition, TransitionV1::Cut);
        }
    }

    #[test]
    fn seed_outro_is_fixed() {
        let loaded = loaded_project_with_steps(3);
        let plan = seed_launch_teaser(&loaded).unwrap();
        assert_eq!(plan.outro_text, "Made with Rollshot");
    }

    #[test]
    fn seed_provenance_has_version_and_no_agent() {
        let loaded = loaded_project_with_steps(3);
        let plan = seed_launch_teaser(&loaded).unwrap();
        assert_eq!(plan.provenance.deterministic_seed_version, 1);
        assert!(plan.provenance.agent.is_none());
        assert!(plan.provenance.repository_reads.is_empty());
        assert!(plan.provenance.accepted_user_edits.is_empty());
    }

    #[test]
    fn seed_is_deterministic() {
        let loaded = loaded_project_with_steps(5);
        let plan1 = seed_launch_teaser(&loaded).unwrap();
        let plan2 = seed_launch_teaser(&loaded).unwrap();
        assert_eq!(plan1, plan2);
    }

    #[test]
    fn binding_rejects_changed_motion_digest() {
        let loaded = loaded_project_with_steps(3);
        let mut plan = seed_launch_teaser(&loaded).unwrap();
        plan.source.motion_sha256 = "f".repeat(64);
        assert_eq!(
            validate_launch_teaser_binding(&plan, &loaded)
                .unwrap_err()
                .category(),
            "stale-motion"
        );
    }

    #[test]
    fn binding_rejects_changed_revision() {
        let loaded = loaded_project_with_steps(3);
        let mut plan = seed_launch_teaser(&loaded).unwrap();
        plan.source.project_revision = 999;
        assert_eq!(
            validate_launch_teaser_binding(&plan, &loaded)
                .unwrap_err()
                .category(),
            "stale-project"
        );
    }

    #[test]
    fn binding_rejects_changed_projection_digest() {
        let loaded = loaded_project_with_steps(3);
        let mut plan = seed_launch_teaser(&loaded).unwrap();
        plan.source.projection_digest = "f".repeat(64);
        assert_eq!(
            validate_launch_teaser_binding(&plan, &loaded)
                .unwrap_err()
                .category(),
            "stale-project"
        );
    }

    #[test]
    fn binding_rejects_missing_step() {
        let loaded = loaded_project_with_steps(3);
        let mut plan = seed_launch_teaser(&loaded).unwrap();
        plan.shots[0].reviewed_step_id = ProjectStepId(999);
        assert_eq!(
            validate_launch_teaser_binding(&plan, &loaded)
                .unwrap_err()
                .category(),
            "missing-step"
        );
    }

    #[test]
    fn binding_passes_for_valid_plan() {
        let loaded = loaded_project_with_steps(3);
        let plan = seed_launch_teaser(&loaded).unwrap();
        assert!(validate_launch_teaser_binding(&plan, &loaded).is_ok());
    }

    #[test]
    fn selected_indices_for_three() {
        assert_eq!(selected_indices(3), vec![0, 1, 2]);
    }

    #[test]
    fn selected_indices_for_five() {
        assert_eq!(selected_indices(5), vec![0, 1, 2, 3, 4]);
    }

    #[test]
    fn selected_indices_for_eight() {
        assert_eq!(selected_indices(8), vec![0, 1, 3, 5, 7]);
    }

    #[test]
    fn selected_indices_for_ten() {
        assert_eq!(selected_indices(10), vec![0, 2, 4, 6, 9]);
    }
}
