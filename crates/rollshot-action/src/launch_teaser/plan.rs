//! Typed launch teaser plan and validation contract.
//!
//! All fields are bounded integers. No serialized floats. No user/model
//! strings enter FFmpeg arguments, filter names, expressions, codecs, or
//! paths. Text is rasterized into PNG overlays before FFmpeg invocation.

use serde::{Deserialize, Serialize};

use crate::project::ProjectStepId;

use super::error::LaunchTeaserError;

// ========================================================================
// Constants
// ========================================================================

pub const LAUNCH_TEASER_SCHEMA_VERSION: u32 = 1;
pub const FINAL_WIDTH: u32 = 1920;
pub const FINAL_HEIGHT: u32 = 1080;
pub const FINAL_FPS: u32 = 30;
pub const PREVIEW_WIDTH: u32 = 960;
pub const PREVIEW_HEIGHT: u32 = 540;
pub const MIN_DURATION_MS: u64 = 15_000;
pub const MAX_DURATION_MS: u64 = 25_000;
pub const MIN_SHOTS: usize = 3;
pub const MAX_SHOTS: usize = 5;
pub const MAX_NORMALIZED_COORD: u16 = 10_000;
pub const MIN_ZOOM_PERMILLE: u16 = 1_000;
pub const MAX_ZOOM_PERMILLE: u16 = 2_000;
pub const MIN_CROSSFADE_MS: u16 = 100;
pub const MAX_CROSSFADE_MS: u16 = 750;
pub const MAX_HOOK_OUTRO_BYTES: usize = 256;
pub const MAX_HOOK_OUTRO_CHARS: usize = 120;
pub const MAX_CAPTION_BYTES: usize = 512;
pub const MAX_CAPTION_CHARS: usize = 240;
pub const OUTRO_DURATION_MS: u64 = 1_500;
pub const PLAN_DOMAIN_SEPARATOR: &[u8] = b"rollshot-launch-teaser-plan-v1\0";
const SHA256_HEX_LEN: usize = 64;

// ========================================================================
// DTOs
// ========================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NormalizedPointV1 {
    pub x: u16,
    pub y: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FocusPathV1 {
    pub start: NormalizedPointV1,
    pub end: NormalizedPointV1,
    pub zoom_permille: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SpeedV1 {
    P750,
    P1000,
    P1250,
    P1500,
    P2000,
}

impl SpeedV1 {
    pub fn permille(&self) -> u64 {
        match self {
            Self::P750 => 750,
            Self::P1000 => 1_000,
            Self::P1250 => 1_250,
            Self::P1500 => 1_500,
            Self::P2000 => 2_000,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum TransitionV1 {
    Cut,
    Crossfade { duration_ms: u16 },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LaunchTeaserSourceV1 {
    pub project_revision: u64,
    pub projection_digest: String,
    pub motion_sha256: String,
    pub motion_duration_ms: u64,
    pub motion_width: u32,
    pub motion_height: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LaunchTeaserShotV1 {
    pub reviewed_step_id: ProjectStepId,
    pub source_start_ms: u64,
    pub source_end_ms: u64,
    pub focus_path: FocusPathV1,
    pub speed: SpeedV1,
    pub caption: String,
    pub transition: TransitionV1,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RepositoryReadProvenanceV1 {
    pub relative_path: String,
    pub content_sha256: String,
    pub bytes_read: u64,
    pub bytes_returned: u64,
    pub truncated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentProvenanceV1 {
    pub run_id: String,
    pub skill_package_digest: String,
    pub authority_snapshot_digest: String,
    pub repository_grant_digest: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AcceptedEditV1 {
    pub field_path: String,
    pub source: AcceptedEditSourceV1,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AcceptedEditSourceV1 {
    DeterministicSeed,
    Agent,
    User,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LaunchTeaserProvenanceV1 {
    pub deterministic_seed_version: u32,
    pub agent: Option<AgentProvenanceV1>,
    pub repository_reads: Vec<RepositoryReadProvenanceV1>,
    pub accepted_user_edits: Vec<AcceptedEditV1>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LaunchTeaserPlanV1 {
    pub schema_version: u32,
    pub source: LaunchTeaserSourceV1,
    pub hook: String,
    pub shots: Vec<LaunchTeaserShotV1>,
    pub outro_text: String,
    pub provenance: LaunchTeaserProvenanceV1,
}

// ========================================================================
// Validated wrapper
// ========================================================================

#[derive(Debug, Clone)]
pub struct ValidatedLaunchTeaserPlan {
    plan: LaunchTeaserPlanV1,
    duration_ms: u64,
}

impl ValidatedLaunchTeaserPlan {
    pub fn plan(&self) -> &LaunchTeaserPlanV1 {
        &self.plan
    }

    pub fn duration_ms(&self) -> u64 {
        self.duration_ms
    }
}

// ========================================================================
// Validation
// ========================================================================

impl LaunchTeaserPlanV1 {
    pub fn validate(&self) -> Result<ValidatedLaunchTeaserPlan, LaunchTeaserError> {
        if self.schema_version != LAUNCH_TEASER_SCHEMA_VERSION {
            return Err(LaunchTeaserError::UnsupportedSchema);
        }

        if self.shots.len() < MIN_SHOTS || self.shots.len() > MAX_SHOTS {
            return Err(LaunchTeaserError::ShotCount);
        }

        for shot in &self.shots {
            validate_shot(shot, self.source.motion_duration_ms)?;
        }

        for i in 0..self.shots.len() {
            let shot = &self.shots[i];
            if shot.source_start_ms >= shot.source_end_ms {
                return Err(LaunchTeaserError::SourceRange);
            }
            if shot.source_end_ms > self.source.motion_duration_ms {
                return Err(LaunchTeaserError::SourceRange);
            }
            if i > 0 {
                let prev = &self.shots[i - 1];
                if shot.source_start_ms < prev.source_end_ms {
                    return Err(LaunchTeaserError::SourceRange);
                }
            }
        }

        validate_text_bound(&self.hook, MAX_HOOK_OUTRO_BYTES, MAX_HOOK_OUTRO_CHARS)?;
        validate_text_bound(&self.outro_text, MAX_HOOK_OUTRO_BYTES, MAX_HOOK_OUTRO_CHARS)?;

        let duration_ms = compute_displayed_duration(&self.shots)?;

        if duration_ms < MIN_DURATION_MS || duration_ms > MAX_DURATION_MS {
            return Err(LaunchTeaserError::Duration);
        }

        if !is_canonical_sha256(&self.source.motion_sha256)
            || !is_canonical_sha256(&self.source.projection_digest)
        {
            return Err(LaunchTeaserError::SourceBinding);
        }

        Ok(ValidatedLaunchTeaserPlan {
            plan: self.clone(),
            duration_ms,
        })
    }
}

fn validate_shot(
    shot: &LaunchTeaserShotV1,
    motion_duration_ms: u64,
) -> Result<(), LaunchTeaserError> {
    validate_normalized_point(shot.focus_path.start)?;
    validate_normalized_point(shot.focus_path.end)?;
    if shot.focus_path.zoom_permille < MIN_ZOOM_PERMILLE
        || shot.focus_path.zoom_permille > MAX_ZOOM_PERMILLE
    {
        return Err(LaunchTeaserError::FocusPath);
    }

    match &shot.transition {
        TransitionV1::Cut => {}
        TransitionV1::Crossfade { duration_ms } => {
            if *duration_ms < MIN_CROSSFADE_MS || *duration_ms > MAX_CROSSFADE_MS {
                return Err(LaunchTeaserError::Transition);
            }
        }
    }

    validate_text_bound(&shot.caption, MAX_CAPTION_BYTES, MAX_CAPTION_CHARS)?;

    if shot.source_start_ms >= shot.source_end_ms {
        return Err(LaunchTeaserError::SourceRange);
    }
    if shot.source_end_ms > motion_duration_ms {
        return Err(LaunchTeaserError::SourceRange);
    }

    Ok(())
}

fn validate_normalized_point(point: NormalizedPointV1) -> Result<(), LaunchTeaserError> {
    if point.x > MAX_NORMALIZED_COORD || point.y > MAX_NORMALIZED_COORD {
        return Err(LaunchTeaserError::FocusPath);
    }
    Ok(())
}

fn validate_text_bound(
    text: &str,
    max_bytes: usize,
    max_chars: usize,
) -> Result<(), LaunchTeaserError> {
    if text.len() > max_bytes || text.chars().count() > max_chars {
        return Err(LaunchTeaserError::Text);
    }
    Ok(())
}

fn compute_displayed_duration(shots: &[LaunchTeaserShotV1]) -> Result<u64, LaunchTeaserError> {
    let mut total_ms: u64 = 0;
    for (i, shot) in shots.iter().enumerate() {
        let source_dur = shot
            .source_end_ms
            .checked_sub(shot.source_start_ms)
            .ok_or(LaunchTeaserError::ArithmeticOverflow)?;
        let displayed = source_dur
            .checked_mul(1_000)
            .ok_or(LaunchTeaserError::ArithmeticOverflow)?
            .checked_div(shot.speed.permille())
            .ok_or(LaunchTeaserError::ArithmeticOverflow)?;
        total_ms = total_ms
            .checked_add(displayed)
            .ok_or(LaunchTeaserError::ArithmeticOverflow)?;

        if i > 0 {
            if let TransitionV1::Crossfade { duration_ms } = shot.transition {
                let overlap = duration_ms as u64;
                total_ms = total_ms
                    .checked_sub(overlap)
                    .ok_or(LaunchTeaserError::ArithmeticOverflow)?;
            }
        }
    }
    Ok(total_ms)
}

fn is_canonical_sha256(digest: &str) -> bool {
    digest.len() == SHA256_HEX_LEN
        && digest
            .as_bytes()
            .iter()
            .all(|b| b.is_ascii_digit() || (*b >= b'a' && *b <= b'f'))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn valid_plan() -> LaunchTeaserPlanV1 {
        LaunchTeaserPlanV1 {
            schema_version: LAUNCH_TEASER_SCHEMA_VERSION,
            source: LaunchTeaserSourceV1 {
                project_revision: 1,
                projection_digest: "a".repeat(64),
                motion_sha256: "b".repeat(64),
                motion_duration_ms: 30_000,
                motion_width: 1920,
                motion_height: 1080,
            },
            hook: "Test Hook".into(),
            shots: vec![
                LaunchTeaserShotV1 {
                    reviewed_step_id: ProjectStepId(1),
                    source_start_ms: 0,
                    source_end_ms: 5_000,
                    focus_path: FocusPathV1 {
                        start: NormalizedPointV1 { x: 5_000, y: 5_000 },
                        end: NormalizedPointV1 { x: 5_000, y: 5_000 },
                        zoom_permille: 1_000,
                    },
                    speed: SpeedV1::P1000,
                    caption: "First step".into(),
                    transition: TransitionV1::Cut,
                },
                LaunchTeaserShotV1 {
                    reviewed_step_id: ProjectStepId(2),
                    source_start_ms: 5_000,
                    source_end_ms: 10_000,
                    focus_path: FocusPathV1 {
                        start: NormalizedPointV1 { x: 5_000, y: 5_000 },
                        end: NormalizedPointV1 { x: 5_000, y: 5_000 },
                        zoom_permille: 1_000,
                    },
                    speed: SpeedV1::P1000,
                    caption: "Second step".into(),
                    transition: TransitionV1::Cut,
                },
                LaunchTeaserShotV1 {
                    reviewed_step_id: ProjectStepId(3),
                    source_start_ms: 10_000,
                    source_end_ms: 15_000,
                    focus_path: FocusPathV1 {
                        start: NormalizedPointV1 { x: 5_000, y: 5_000 },
                        end: NormalizedPointV1 { x: 5_000, y: 5_000 },
                        zoom_permille: 1_000,
                    },
                    speed: SpeedV1::P1000,
                    caption: "Third step".into(),
                    transition: TransitionV1::Cut,
                },
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
    fn valid_three_shot_plan_reports_exact_duration() {
        let plan = valid_plan();
        let validated = plan.validate().unwrap();
        assert_eq!(validated.duration_ms(), 15_000);
    }

    #[test]
    fn plan_rejects_two_shots() {
        let mut plan = valid_plan();
        plan.shots.pop();
        assert_eq!(plan.validate().unwrap_err().category(), "shot-count");
    }

    #[test]
    fn plan_rejects_overlapping_source_ranges() {
        let mut plan = valid_plan();
        plan.shots[1].source_start_ms = plan.shots[0].source_end_ms - 1;
        assert_eq!(plan.validate().unwrap_err().category(), "source-range");
    }

    #[test]
    fn unknown_json_field_is_rejected() {
        let mut value = serde_json::to_value(valid_plan()).unwrap();
        value
            .as_object_mut()
            .unwrap()
            .insert("filtergraph".into(), serde_json::json!("evil"));
        assert!(serde_json::from_value::<LaunchTeaserPlanV1>(value).is_err());
    }

    #[test]
    fn rejects_wrong_schema_version() {
        let mut plan = valid_plan();
        plan.schema_version = 99;
        assert_eq!(plan.validate().unwrap_err().category(), "unsupported-schema");
    }

    #[test]
    fn rejects_six_shots() {
        let mut plan = valid_plan();
        for i in 4..=6 {
            let mut shot = plan.shots[0].clone();
            shot.reviewed_step_id = ProjectStepId(i);
            shot.source_start_ms = 15_000 + (i as u64 - 4) * 2_500;
            shot.source_end_ms = shot.source_start_ms + 2_500;
            plan.shots.push(shot);
        }
        plan.source.motion_duration_ms = 30_000;
        assert_eq!(plan.validate().unwrap_err().category(), "shot-count");
    }

    #[test]
    fn valid_five_shot_plan() {
        let mut plan = valid_plan();
        plan.shots.clear();
        for i in 1..=5 {
            let start = (i as u64 - 1) * 3_500;
            plan.shots.push(LaunchTeaserShotV1 {
                reviewed_step_id: ProjectStepId(i),
                source_start_ms: start,
                source_end_ms: start + 3_500,
                focus_path: FocusPathV1 {
                    start: NormalizedPointV1 { x: 5_000, y: 5_000 },
                    end: NormalizedPointV1 { x: 5_000, y: 5_000 },
                    zoom_permille: 1_000,
                },
                speed: SpeedV1::P1000,
                caption: format!("Step {i}"),
                transition: TransitionV1::Cut,
            });
        }
        plan.source.motion_duration_ms = 17_500;
        let validated = plan.validate().unwrap();
        assert_eq!(validated.duration_ms(), 17_500);
    }

    #[test]
    fn rejects_source_start_at_motion_end() {
        let mut plan = valid_plan();
        plan.shots[0].source_start_ms = plan.source.motion_duration_ms;
        assert_eq!(plan.validate().unwrap_err().category(), "source-range");
    }

    #[test]
    fn rejects_focus_coordinate_above_bound() {
        let mut plan = valid_plan();
        plan.shots[0].focus_path.start.x = 10_001;
        assert_eq!(plan.validate().unwrap_err().category(), "focus-path");
    }

    #[test]
    fn accepts_focus_coordinate_at_bound() {
        let mut plan = valid_plan();
        plan.shots[0].focus_path.start.x = 10_000;
        assert!(plan.validate().is_ok());
    }

    #[test]
    fn rejects_zoom_below_bound() {
        let mut plan = valid_plan();
        plan.shots[0].focus_path.zoom_permille = 999;
        assert_eq!(plan.validate().unwrap_err().category(), "focus-path");
    }

    #[test]
    fn rejects_zoom_above_bound() {
        let mut plan = valid_plan();
        plan.shots[0].focus_path.zoom_permille = 2_001;
        assert_eq!(plan.validate().unwrap_err().category(), "focus-path");
    }

    #[test]
    fn rejects_crossfade_below_minimum() {
        let mut plan = valid_plan();
        plan.shots[1].transition = TransitionV1::Crossfade { duration_ms: 99 };
        assert_eq!(plan.validate().unwrap_err().category(), "transition");
    }

    #[test]
    fn rejects_crossfade_above_maximum() {
        let mut plan = valid_plan();
        plan.shots[1].transition = TransitionV1::Crossfade { duration_ms: 751 };
        assert_eq!(plan.validate().unwrap_err().category(), "transition");
    }

    #[test]
    fn rejects_hook_too_long_bytes() {
        let mut plan = valid_plan();
        plan.hook = "x".repeat(257);
        assert_eq!(plan.validate().unwrap_err().category(), "text");
    }

    #[test]
    fn rejects_hook_too_long_chars() {
        let mut plan = valid_plan();
        plan.hook = "\u{e9}".repeat(121);
        assert_eq!(plan.validate().unwrap_err().category(), "text");
    }

    #[test]
    fn rejects_caption_too_long() {
        let mut plan = valid_plan();
        plan.shots[0].caption = "x".repeat(513);
        assert_eq!(plan.validate().unwrap_err().category(), "text");
    }

    #[test]
    fn rejects_outro_too_long() {
        let mut plan = valid_plan();
        plan.outro_text = "x".repeat(257);
        assert_eq!(plan.validate().unwrap_err().category(), "text");
    }

    #[test]
    fn rejects_non_canonical_motion_sha256() {
        let mut plan = valid_plan();
        plan.source.motion_sha256 = "A".repeat(64);
        assert_eq!(plan.validate().unwrap_err().category(), "source-binding");
    }

    #[test]
    fn rejects_non_canonical_projection_digest() {
        let mut plan = valid_plan();
        plan.source.projection_digest = "G".repeat(64);
        assert_eq!(plan.validate().unwrap_err().category(), "source-binding");
    }

    #[test]
    fn rejects_short_sha256() {
        let mut plan = valid_plan();
        plan.source.motion_sha256 = "a".repeat(63);
        assert_eq!(plan.validate().unwrap_err().category(), "source-binding");
    }

    #[test]
    fn crossfade_subtracts_overlap() {
        let mut plan = valid_plan();
        plan.shots[1].transition = TransitionV1::Crossfade { duration_ms: 500 };
        assert_eq!(plan.validate().unwrap_err().category(), "duration");
    }

    #[test]
    fn crossfade_with_extended_source_stays_in_range() {
        let mut plan = valid_plan();
        plan.shots[1].transition = TransitionV1::Crossfade { duration_ms: 500 };
        plan.shots[0].source_end_ms = 5_500;
        plan.shots[1].source_start_ms = 5_500;
        plan.shots[1].source_end_ms = 11_000;
        plan.shots[2].source_start_ms = 11_000;
        plan.shots[2].source_end_ms = 16_500;
        plan.source.motion_duration_ms = 16_500;
        let validated = plan.validate().unwrap();
        assert_eq!(validated.duration_ms(), 16_000);
    }

    #[test]
    fn rejects_duration_below_minimum() {
        let mut plan = valid_plan();
        plan.shots[0].source_end_ms = 4_000;
        plan.shots[1].source_start_ms = 4_000;
        plan.shots[1].source_end_ms = 8_000;
        plan.shots[2].source_start_ms = 8_000;
        plan.shots[2].source_end_ms = 12_000;
        plan.source.motion_duration_ms = 12_000;
        assert_eq!(plan.validate().unwrap_err().category(), "duration");
    }

    #[test]
    fn rejects_duration_above_maximum() {
        let mut plan = valid_plan();
        plan.shots[0].source_end_ms = 10_000;
        plan.shots[1].source_start_ms = 10_000;
        plan.shots[1].source_end_ms = 20_000;
        plan.shots[2].source_start_ms = 20_000;
        plan.shots[2].source_end_ms = 30_000;
        plan.source.motion_duration_ms = 30_000;
        assert_eq!(plan.validate().unwrap_err().category(), "duration");
    }

    #[test]
    fn slow_speed_increases_displayed_duration() {
        let mut plan = valid_plan();
        plan.shots[0].speed = SpeedV1::P750;
        let validated = plan.validate().unwrap();
        assert_eq!(validated.duration_ms(), 16_666);
    }

    #[test]
    fn fast_speed_decreases_displayed_duration() {
        let mut plan = valid_plan();
        plan.shots[0].speed = SpeedV1::P2000;
        plan.shots[1].speed = SpeedV1::P2000;
        plan.shots[2].speed = SpeedV1::P2000;
        assert_eq!(plan.validate().unwrap_err().category(), "duration");
    }

    #[test]
    fn validated_plan_exposes_inner_plan() {
        let plan = valid_plan();
        let validated = plan.validate().unwrap();
        assert_eq!(validated.plan().schema_version, LAUNCH_TEASER_SCHEMA_VERSION);
        assert_eq!(validated.plan().shots.len(), 3);
    }

    #[test]
    fn speed_permille_values() {
        assert_eq!(SpeedV1::P750.permille(), 750);
        assert_eq!(SpeedV1::P1000.permille(), 1_000);
        assert_eq!(SpeedV1::P1250.permille(), 1_250);
        assert_eq!(SpeedV1::P1500.permille(), 1_500);
        assert_eq!(SpeedV1::P2000.permille(), 2_000);
    }
}
