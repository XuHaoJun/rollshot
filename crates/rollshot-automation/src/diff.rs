use serde::{Deserialize, Serialize};

use crate::{CapabilityName, IrNodeKind, WorkflowIr};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SemanticSummary {
    pub lines: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SemanticDiff {
    pub changes: Vec<SemanticChange>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SemanticChange {
    CapabilityAdded {
        capability: CapabilityName,
    },
    CapabilityRemoved {
        capability: CapabilityName,
    },
    CapabilityLimitChanged {
        capability: CapabilityName,
        before: u32,
        after: u32,
    },
    EditKindAdded {
        kind: crate::ProposedEditKind,
    },
    EditKindRemoved {
        kind: crate::ProposedEditKind,
    },
    MaxOutputCandidatesChanged {
        before: u32,
        after: u32,
    },
    StaticCostChanged {
        before_steps: u32,
        after_steps: u32,
    },
    ConditionChanged {
        before: String,
        after: String,
    },
    TransformChanged {
        before: String,
        after: String,
    },
}

pub fn semantic_summary(ir: &WorkflowIr) -> SemanticSummary {
    let mut lines = ir
        .capability_manifest
        .calls
        .iter()
        .map(|call| {
            format!(
                "{}: at most {} call(s), {} result(s) per call",
                format!("{:?}", call.capability).to_lowercase(),
                call.max_calls,
                call.max_results_per_call
            )
        })
        .collect::<Vec<_>>();
    lines.push(format!(
        "at most {} output candidate(s)",
        ir.static_cost.max_output_candidates
    ));
    for node in &ir.nodes {
        match &node.kind {
            IrNodeKind::Condition(condition) => {
                lines.push(format!("condition: {}", condition.expression_summary));
            }
            IrNodeKind::Transform(transform) => {
                lines.push(format!("transform: {}", transform.expression_summary));
            }
            _ => {}
        }
    }
    lines.push(format!("possible edit kinds: {:?}", ir.possible_edit_kinds));
    SemanticSummary { lines }
}

pub fn semantic_diff(before: &WorkflowIr, after: &WorkflowIr) -> SemanticDiff {
    let limits = |ir: &WorkflowIr| {
        ir.nodes
            .iter()
            .filter_map(|node| match &node.kind {
                IrNodeKind::CapabilityCall(call) => Some((call.capability, call.literal_limit)),
                _ => None,
            })
            .collect::<std::collections::BTreeMap<_, _>>()
    };
    let before_limits = limits(before);
    let after_limits = limits(after);
    let mut changes = Vec::new();

    for (capability, limit) in &before_limits {
        match after_limits.get(capability) {
            None => changes.push(SemanticChange::CapabilityRemoved {
                capability: *capability,
            }),
            Some(after_limit) if after_limit != limit => {
                changes.push(SemanticChange::CapabilityLimitChanged {
                    capability: *capability,
                    before: *limit,
                    after: *after_limit,
                });
            }
            Some(_) => {}
        }
    }
    for capability in after_limits.keys() {
        if !before_limits.contains_key(capability) {
            changes.push(SemanticChange::CapabilityAdded {
                capability: *capability,
            });
        }
    }
    for kind in before
        .possible_edit_kinds
        .difference(&after.possible_edit_kinds)
    {
        changes.push(SemanticChange::EditKindRemoved { kind: *kind });
    }
    for kind in after
        .possible_edit_kinds
        .difference(&before.possible_edit_kinds)
    {
        changes.push(SemanticChange::EditKindAdded { kind: *kind });
    }
    if before.static_cost.max_output_candidates != after.static_cost.max_output_candidates {
        changes.push(SemanticChange::MaxOutputCandidatesChanged {
            before: before.static_cost.max_output_candidates,
            after: after.static_cost.max_output_candidates,
        });
    }
    if before.nodes.len() != after.nodes.len() {
        changes.push(SemanticChange::StaticCostChanged {
            before_steps: before.nodes.len() as u32,
            after_steps: after.nodes.len() as u32,
        });
    }
    let conditions = |ir: &WorkflowIr| {
        ir.nodes
            .iter()
            .filter_map(|node| match &node.kind {
                IrNodeKind::Condition(value) => Some(value.expression_summary.clone()),
                _ => None,
            })
            .collect::<Vec<_>>()
    };
    let transforms = |ir: &WorkflowIr| {
        ir.nodes
            .iter()
            .filter_map(|node| match &node.kind {
                IrNodeKind::Transform(value) => Some(value.expression_summary.clone()),
                _ => None,
            })
            .collect::<Vec<_>>()
    };
    for (before, after) in conditions(before).into_iter().zip(conditions(after)) {
        if before != after {
            changes.push(SemanticChange::ConditionChanged { before, after });
        }
    }
    for (before, after) in transforms(before).into_iter().zip(transforms(after)) {
        if before != after {
            changes.push(SemanticChange::TransformChanged { before, after });
        }
    }
    SemanticDiff { changes }
}
