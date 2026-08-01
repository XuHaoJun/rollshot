//! Launch-teaser provider-neutral patch schema and terminal tool.
//!
//! The agent skill proposes changes through a strict `LaunchTeaserPatchV1`.
//! Product code maps patches onto domain plans; the agent never renders
//! or mutates plans directly. `rollshot-agent` must not depend on
//! `rollshot-action`.

use serde::{Deserialize, Serialize};

use crate::model::ToolDefinition;
use crate::runtime::RunBudget;

// ========================================================================
// Constants (matching domain plan ceilings)
// ========================================================================

const MAX_NORMALIZED_COORD: u16 = 10_000;
const MIN_ZOOM_PERMILLE: u16 = 1_000;
const MAX_ZOOM_PERMILLE: u16 = 2_000;
const MIN_CROSSFADE_MS: u16 = 100;
const MAX_CROSSFADE_MS: u16 = 750;
const MAX_HOOK_OUTRO_BYTES: usize = 256;
const MAX_HOOK_OUTRO_CHARS: usize = 120;
const MAX_CAPTION_BYTES: usize = 512;
const MAX_CAPTION_CHARS: usize = 240;
const MIN_SHOTS: usize = 3;
const MAX_SHOTS: usize = 5;
const ALLOWED_SPEED_PERMILLE: &[u16] = &[750, 1_000, 1_250, 1_500, 2_000];

// ========================================================================
// Errors
// ========================================================================

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum LaunchTeaserPatchError {
    #[error("shot_order must contain 3-5 unique step IDs")]
    ShotOrderCount,
    #[error("shots must contain at most one patch per ordered step ID")]
    DuplicateShotId,
    #[error("shot_order and shots must reference the same step IDs")]
    ShotOrderMismatch,
    #[error("unknown fields are not permitted")]
    UnknownField,
    #[error("invalid field: {0}")]
    InvalidField(String),
    #[error("unsupported speed permille: {0}")]
    UnsupportedSpeed(u16),
    #[error("unsupported transition: {0}")]
    UnsupportedTransition(String),
    #[error("text exceeds byte limit: {len} > {max}")]
    TextTooLong { len: usize, max: usize },
    #[error("text exceeds char limit: {len} > {max}")]
    TextTooManyChars { len: usize, max: usize },
    #[error("deserialization failed: {0}")]
    Deserialization(String),
}

// ========================================================================
// Transition patch
// ========================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum LaunchTeaserTransitionPatchV1 {
    Cut,
    Crossfade { duration_ms: u16 },
}

// ========================================================================
// Shot patch
// ========================================================================

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LaunchTeaserShotPatchV1 {
    pub reviewed_step_id: u64,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub source_start_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub source_end_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub focus_start_x: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub focus_start_y: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub focus_end_x: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub focus_end_y: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub zoom_permille: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub speed_permille: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub caption: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub transition: Option<LaunchTeaserTransitionPatchV1>,
}

// ========================================================================
// Top-level patch
// ========================================================================

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LaunchTeaserPatchV1 {
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub hook: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub outro_text: Option<String>,
    pub shot_order: Vec<u64>,
    pub shots: Vec<LaunchTeaserShotPatchV1>,
}

// ========================================================================
// Validation
// ========================================================================

/// Parse and validate a launch-teaser patch from a JSON value.
///
/// Applies the same numeric and text ceilings as the domain plan.
/// Product mapping performs final source/project/duration validation.
pub fn parse_launch_teaser_patch(
    value: &serde_json::Value,
) -> Result<LaunchTeaserPatchV1, LaunchTeaserPatchError> {
    let patch: LaunchTeaserPatchV1 = serde_json::from_value(value.clone())
        .map_err(|e| LaunchTeaserPatchError::Deserialization(e.to_string()))?;

    // Validate shot_order: 3-5 unique IDs.
    if patch.shot_order.len() < MIN_SHOTS || patch.shot_order.len() > MAX_SHOTS {
        return Err(LaunchTeaserPatchError::ShotOrderCount);
    }
    {
        let mut seen = std::collections::HashSet::new();
        for id in &patch.shot_order {
            if !seen.insert(id) {
                return Err(LaunchTeaserPatchError::DuplicateShotId);
            }
        }
    }

    // Validate shots: at most one per ordered ID, and IDs must match.
    if patch.shots.len() > patch.shot_order.len() {
        return Err(LaunchTeaserPatchError::DuplicateShotId);
    }
    {
        let mut seen = std::collections::HashSet::new();
        for shot in &patch.shots {
            if !seen.insert(shot.reviewed_step_id) {
                return Err(LaunchTeaserPatchError::DuplicateShotId);
            }
        }
        // All shot IDs must appear in shot_order.
        let order_set: std::collections::HashSet<u64> = patch.shot_order.iter().copied().collect();
        for shot in &patch.shots {
            if !order_set.contains(&shot.reviewed_step_id) {
                return Err(LaunchTeaserPatchError::ShotOrderMismatch);
            }
        }
    }

    // Validate text fields.
    validate_optional_text(
        patch.hook.as_deref(),
        MAX_HOOK_OUTRO_BYTES,
        MAX_HOOK_OUTRO_CHARS,
    )?;
    validate_optional_text(
        patch.outro_text.as_deref(),
        MAX_HOOK_OUTRO_BYTES,
        MAX_HOOK_OUTRO_CHARS,
    )?;

    // Validate per-shot fields.
    for shot in &patch.shots {
        validate_shot_patch(shot)?;
    }

    Ok(patch)
}

fn validate_optional_text(
    text: Option<&str>,
    max_bytes: usize,
    max_chars: usize,
) -> Result<(), LaunchTeaserPatchError> {
    if let Some(t) = text {
        let bytes = t.len();
        if bytes > max_bytes {
            return Err(LaunchTeaserPatchError::TextTooLong {
                len: bytes,
                max: max_bytes,
            });
        }
        let chars = t.chars().count();
        if chars > max_chars {
            return Err(LaunchTeaserPatchError::TextTooManyChars {
                len: chars,
                max: max_chars,
            });
        }
    }
    Ok(())
}

fn validate_shot_patch(shot: &LaunchTeaserShotPatchV1) -> Result<(), LaunchTeaserPatchError> {
    // Focus coordinates.
    if let Some(x) = shot.focus_start_x {
        if x > MAX_NORMALIZED_COORD {
            return Err(LaunchTeaserPatchError::InvalidField(format!(
                "focus_start_x {x} > {MAX_NORMALIZED_COORD}"
            )));
        }
    }
    if let Some(y) = shot.focus_start_y {
        if y > MAX_NORMALIZED_COORD {
            return Err(LaunchTeaserPatchError::InvalidField(format!(
                "focus_start_y {y} > {MAX_NORMALIZED_COORD}"
            )));
        }
    }
    if let Some(x) = shot.focus_end_x {
        if x > MAX_NORMALIZED_COORD {
            return Err(LaunchTeaserPatchError::InvalidField(format!(
                "focus_end_x {x} > {MAX_NORMALIZED_COORD}"
            )));
        }
    }
    if let Some(y) = shot.focus_end_y {
        if y > MAX_NORMALIZED_COORD {
            return Err(LaunchTeaserPatchError::InvalidField(format!(
                "focus_end_y {y} > {MAX_NORMALIZED_COORD}"
            )));
        }
    }

    // Zoom.
    if let Some(z) = shot.zoom_permille {
        if !(MIN_ZOOM_PERMILLE..=MAX_ZOOM_PERMILLE).contains(&z) {
            return Err(LaunchTeaserPatchError::InvalidField(format!(
                "zoom_permille {z} not in {MIN_ZOOM_PERMILLE}..={MAX_ZOOM_PERMILLE}"
            )));
        }
    }

    // Speed.
    if let Some(s) = shot.speed_permille {
        if !ALLOWED_SPEED_PERMILLE.contains(&s) {
            return Err(LaunchTeaserPatchError::UnsupportedSpeed(s));
        }
    }

    // Caption.
    validate_optional_text(
        shot.caption.as_deref(),
        MAX_CAPTION_BYTES,
        MAX_CAPTION_CHARS,
    )?;

    // Transition.
    if let Some(t) = &shot.transition {
        match t {
            LaunchTeaserTransitionPatchV1::Cut => {}
            LaunchTeaserTransitionPatchV1::Crossfade { duration_ms } => {
                if !(MIN_CROSSFADE_MS..=MAX_CROSSFADE_MS).contains(duration_ms) {
                    return Err(LaunchTeaserPatchError::UnsupportedTransition(format!(
                        "crossfade duration_ms {duration_ms} not in {MIN_CROSSFADE_MS}..={MAX_CROSSFADE_MS}"
                    )));
                }
            }
        }
    }

    Ok(())
}

// ========================================================================
// Terminal tool definition
// ========================================================================

/// The terminal tool name for launch-teaser submission.
pub const SUBMIT_LAUNCH_TEASER_PLAN_TOOL_NAME: &str = "submit_launch_teaser_plan";

/// Create the tool definition for the terminal launch-teaser tool.
pub fn launch_teaser_submit_definition() -> ToolDefinition {
    ToolDefinition {
        name: SUBMIT_LAUNCH_TEASER_PLAN_TOOL_NAME.to_string(),
        description: "Submit a launch teaser plan patch for review.".to_string(),
        parameters: serde_json::json!({
            "type": "object",
            "properties": {
                "hook": {
                    "type": ["string", "null"],
                    "description": "Hook text override (max 256 bytes, 120 chars)."
                },
                "outro_text": {
                    "type": ["string", "null"],
                    "description": "Outro text override (max 256 bytes, 120 chars)."
                },
                "shot_order": {
                    "type": "array",
                    "items": { "type": "integer" },
                    "minItems": 3,
                    "maxItems": 5,
                    "description": "Ordered list of 3-5 unique reviewed step IDs."
                },
                "shots": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {
                            "reviewed_step_id": { "type": "integer" },
                            "source_start_ms": { "type": ["integer", "null"] },
                            "source_end_ms": { "type": ["integer", "null"] },
                            "focus_start_x": { "type": ["integer", "null"] },
                            "focus_start_y": { "type": ["integer", "null"] },
                            "focus_end_x": { "type": ["integer", "null"] },
                            "focus_end_y": { "type": ["integer", "null"] },
                            "zoom_permille": { "type": ["integer", "null"] },
                            "speed_permille": { "type": ["integer", "null"] },
                            "caption": { "type": ["string", "null"] },
                            "transition": {
                                "oneOf": [
                                    {
                                        "type": "object",
                                        "properties": { "kind": { "const": "cut" } },
                                        "required": ["kind"],
                                        "additionalProperties": false
                                    },
                                    {
                                        "type": "object",
                                        "properties": {
                                            "kind": { "const": "crossfade" },
                                            "duration_ms": { "type": "integer" }
                                        },
                                        "required": ["kind", "duration_ms"],
                                        "additionalProperties": false
                                    }
                                ]
                            }
                        },
                        "required": ["reviewed_step_id"],
                        "additionalProperties": false
                    },
                    "description": "Per-shot patches (at most one per ordered step ID)."
                }
            },
            "required": ["shot_order", "shots"],
            "additionalProperties": false
        }),
    }
}

// ========================================================================
// Budget
// ========================================================================

/// Create a bounded run budget for a launch-teaser agent run.
/// Allows repository auxiliary calls within fixed ceilings.
pub fn launch_teaser_run_budget() -> RunBudget {
    RunBudget {
        wall_time: std::time::Duration::from_secs(120),
        model_calls: 6,
        attachments: 0,
        tool_calls: 10,
        argument_bytes: 128 * 1024,
        result_bytes: 512 * 1024,
        input_tokens: 50_000,
        output_tokens: 50_000,
        ..RunBudget::unlimited()
    }
}

// ========================================================================
// Tests
// ========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn minimal_patch() -> serde_json::Value {
        serde_json::json!({
            "hook": null,
            "outro_text": null,
            "shot_order": [1, 2, 3],
            "shots": [
                {"reviewed_step_id": 1},
                {"reviewed_step_id": 2},
                {"reviewed_step_id": 3}
            ]
        })
    }

    #[test]
    fn minimal_patch_parses() {
        let patch = parse_launch_teaser_patch(&minimal_patch()).unwrap();
        assert_eq!(patch.shot_order, vec![1, 2, 3]);
        assert_eq!(patch.shots.len(), 3);
        assert!(patch.hook.is_none());
        assert!(patch.outro_text.is_none());
    }

    #[test]
    fn hook_and_outro_changes_accepted() {
        let mut v = minimal_patch();
        v["hook"] = serde_json::json!("Watch this!");
        v["outro_text"] = serde_json::json!("Thanks for watching.");
        let patch = parse_launch_teaser_patch(&v).unwrap();
        assert_eq!(patch.hook.as_deref(), Some("Watch this!"));
        assert_eq!(patch.outro_text.as_deref(), Some("Thanks for watching."));
    }

    #[test]
    fn per_shot_focus_and_speed_accepted() {
        let v = serde_json::json!({
            "hook": null,
            "outro_text": null,
            "shot_order": [1, 2, 3],
            "shots": [
                {
                    "reviewed_step_id": 1,
                    "focus_start_x": 5000,
                    "focus_start_y": 3000,
                    "focus_end_x": 7000,
                    "focus_end_y": 4000,
                    "zoom_permille": 1500,
                    "speed_permille": 1000,
                    "caption": "Step one",
                    "transition": {"kind": "crossfade", "duration_ms": 300}
                },
                {"reviewed_step_id": 2},
                {"reviewed_step_id": 3}
            ]
        });
        let patch = parse_launch_teaser_patch(&v).unwrap();
        assert_eq!(patch.shots[0].focus_start_x, Some(5000));
        assert_eq!(patch.shots[0].speed_permille, Some(1000));
    }

    #[test]
    fn arbitrary_filtergraph_is_rejected() {
        let v = serde_json::json!({
            "hook": null,
            "outro_text": null,
            "shot_order": [1, 2, 3],
            "shots": [{"reviewed_step_id": 1, "filtergraph": "movie=/etc/passwd"}]
        });
        assert!(parse_launch_teaser_patch(&v).is_err());
    }

    #[test]
    fn duplicate_step_ids_rejected() {
        let v = serde_json::json!({
            "hook": null,
            "outro_text": null,
            "shot_order": [1, 1, 2],
            "shots": [
                {"reviewed_step_id": 1},
                {"reviewed_step_id": 2}
            ]
        });
        assert!(parse_launch_teaser_patch(&v).is_err());
    }

    #[test]
    fn too_few_shots_rejected() {
        let v = serde_json::json!({
            "hook": null,
            "outro_text": null,
            "shot_order": [1, 2],
            "shots": [
                {"reviewed_step_id": 1},
                {"reviewed_step_id": 2}
            ]
        });
        assert!(parse_launch_teaser_patch(&v).is_err());
    }

    #[test]
    fn too_many_shots_rejected() {
        let v = serde_json::json!({
            "hook": null,
            "outro_text": null,
            "shot_order": [1, 2, 3, 4, 5, 6],
            "shots": [
                {"reviewed_step_id": 1},
                {"reviewed_step_id": 2},
                {"reviewed_step_id": 3},
                {"reviewed_step_id": 4},
                {"reviewed_step_id": 5},
                {"reviewed_step_id": 6}
            ]
        });
        assert!(parse_launch_teaser_patch(&v).is_err());
    }

    #[test]
    fn unknown_fields_rejected() {
        let v = serde_json::json!({
            "hook": null,
            "outro_text": null,
            "shot_order": [1, 2, 3],
            "shots": [{"reviewed_step_id": 1}],
            "evil": true
        });
        assert!(parse_launch_teaser_patch(&v).is_err());
    }

    #[test]
    fn unsupported_speed_rejected() {
        let v = serde_json::json!({
            "hook": null,
            "outro_text": null,
            "shot_order": [1, 2, 3],
            "shots": [
                {"reviewed_step_id": 1, "speed_permille": 999},
                {"reviewed_step_id": 2},
                {"reviewed_step_id": 3}
            ]
        });
        assert!(parse_launch_teaser_patch(&v).is_err());
    }

    #[test]
    fn unsupported_crossfade_range_rejected() {
        let v = serde_json::json!({
            "hook": null,
            "outro_text": null,
            "shot_order": [1, 2, 3],
            "shots": [
                {"reviewed_step_id": 1, "transition": {"kind": "crossfade", "duration_ms": 50}},
                {"reviewed_step_id": 2},
                {"reviewed_step_id": 3}
            ]
        });
        assert!(parse_launch_teaser_patch(&v).is_err());
    }

    #[test]
    fn oversized_hook_text_rejected() {
        let v = serde_json::json!({
            "hook": "x".repeat(300),
            "outro_text": null,
            "shot_order": [1, 2, 3],
            "shots": [
                {"reviewed_step_id": 1},
                {"reviewed_step_id": 2},
                {"reviewed_step_id": 3}
            ]
        });
        assert!(parse_launch_teaser_patch(&v).is_err());
    }

    #[test]
    fn focus_coordinate_out_of_range_rejected() {
        let v = serde_json::json!({
            "hook": null,
            "outro_text": null,
            "shot_order": [1, 2, 3],
            "shots": [
                {"reviewed_step_id": 1, "focus_start_x": 10001},
                {"reviewed_step_id": 2},
                {"reviewed_step_id": 3}
            ]
        });
        assert!(parse_launch_teaser_patch(&v).is_err());
    }

    #[test]
    fn zoom_out_of_range_rejected() {
        let v = serde_json::json!({
            "hook": null,
            "outro_text": null,
            "shot_order": [1, 2, 3],
            "shots": [
                {"reviewed_step_id": 1, "zoom_permille": 500},
                {"reviewed_step_id": 2},
                {"reviewed_step_id": 3}
            ]
        });
        assert!(parse_launch_teaser_patch(&v).is_err());
    }

    #[test]
    fn shot_order_mismatch_rejected() {
        let v = serde_json::json!({
            "hook": null,
            "outro_text": null,
            "shot_order": [1, 2, 3],
            "shots": [
                {"reviewed_step_id": 1},
                {"reviewed_step_id": 2},
                {"reviewed_step_id": 4}  // not in shot_order
            ]
        });
        assert!(parse_launch_teaser_patch(&v).is_err());
    }

    #[test]
    fn cut_transition_accepted() {
        let v = serde_json::json!({
            "hook": null,
            "outro_text": null,
            "shot_order": [1, 2, 3],
            "shots": [
                {"reviewed_step_id": 1, "transition": {"kind": "cut"}},
                {"reviewed_step_id": 2},
                {"reviewed_step_id": 3}
            ]
        });
        let patch = parse_launch_teaser_patch(&v).unwrap();
        assert_eq!(
            patch.shots[0].transition,
            Some(LaunchTeaserTransitionPatchV1::Cut)
        );
    }

    #[test]
    fn tool_definition_has_strict_schema() {
        let def = launch_teaser_submit_definition();
        assert_eq!(def.name, "submit_launch_teaser_plan");
        let params = &def.parameters;
        assert_eq!(params["additionalProperties"], false);
        assert!(params["required"]
            .as_array()
            .unwrap()
            .contains(&serde_json::json!("shot_order")));
    }

    #[test]
    fn budget_has_fixed_ceilings() {
        let budget = launch_teaser_run_budget();
        assert!(budget.model_calls > 0);
        assert!(budget.tool_calls > 0);
        assert!(budget.wall_time.as_secs() > 0);
        assert!(budget.argument_bytes > 0);
        assert!(budget.result_bytes > 0);
    }
}
