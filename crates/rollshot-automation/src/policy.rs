use std::collections::{BTreeMap, BTreeSet};
use std::time::Duration;

use rollshot_edit_proposal::{PolicyLimits, ProposalId, Provenance};
use rollshot_image_document::AnnotationId;
use serde::{Deserialize, Serialize};

use crate::CapabilityName;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ProposedEditKind {
    AddRedaction,
    AddTextNote,
    AddNumberCallout,
    UpdateRedactionBounds,
    UpdateTextPosition,
    UpdateText,
    UpdateNumberPoints,
    Delete,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ValidationLimits {
    pub max_source_bytes: usize,
    pub max_ast_nodes: u32,
    pub max_literal_bytes: usize,
    pub max_helpers: u32,
    pub max_helper_call_depth: u32,
    pub max_capability_calls: u32,
    pub max_collection_traversals: u32,
    pub max_candidates: u32,
    pub max_output_bytes: usize,
    pub max_input_annotations: u32,
}

impl Default for ValidationLimits {
    fn default() -> Self {
        Self {
            max_source_bytes: 64 * 1024,
            max_ast_nodes: 10_000,
            max_literal_bytes: 32 * 1024,
            max_helpers: 32,
            max_helper_call_depth: 16,
            max_capability_calls: 32,
            max_collection_traversals: 64,
            max_candidates: 1_000,
            max_output_bytes: 1024 * 1024,
            max_input_annotations: 1_000,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ExecutionPolicy {
    pub max_wall_time: Duration,
    pub max_memory_bytes: usize,
    pub max_stack_bytes: usize,
    pub max_capability_calls: u32,
    pub max_calls_by_capability: BTreeMap<CapabilityName, u32>,
    pub max_host_allocation_bytes: usize,
    pub max_output_bytes: usize,
    pub proposal_limits: PolicyLimits,
    pub allowed_edit_kinds: BTreeSet<ProposedEditKind>,
    pub allowed_annotation_ids: BTreeSet<AnnotationId>,
}

impl ExecutionPolicy {
    pub fn smart_redaction_default(
        max_wall_time: Duration,
        max_memory_bytes: usize,
        max_stack_bytes: usize,
    ) -> Self {
        Self {
            max_wall_time,
            max_memory_bytes,
            max_stack_bytes,
            max_capability_calls: 16,
            max_calls_by_capability: BTreeMap::new(),
            max_host_allocation_bytes: 4 * 1024 * 1024,
            max_output_bytes: 1024 * 1024,
            proposal_limits: PolicyLimits {
                max_candidates: 1_000,
                max_total_area_fraction: 1.0,
                allow_out_of_bounds: false,
            },
            allowed_edit_kinds: BTreeSet::from([ProposedEditKind::AddRedaction]),
            allowed_annotation_ids: BTreeSet::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProposalContext {
    pub proposal_id: ProposalId,
    pub base_document_state_id: u64,
    pub provenance: Provenance,
}
