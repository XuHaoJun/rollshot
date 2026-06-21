use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::{
    CapabilityManifest, CapabilityName, IrSchemaVersion, ProposedEditKind, SourceSpan, IR_SCHEMA_V1,
};

pub type NodeId = u32;
pub type FunctionId = u32;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowIr {
    pub ir_schema_version: IrSchemaVersion,
    pub entry: FunctionId,
    pub helpers: Vec<IrFunction>,
    pub nodes: Vec<IrNode>,
    pub output: NodeId,
    pub capability_manifest: CapabilityManifest,
    pub static_cost: StaticCost,
    pub possible_edit_kinds: BTreeSet<ProposedEditKind>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IrFunction {
    pub id: FunctionId,
    pub name: String,
    pub source_span: SourceSpan,
    pub max_call_depth: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IrNode {
    pub id: NodeId,
    pub kind: IrNodeKind,
    pub source_span: SourceSpan,
    pub max_cardinality: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum IrNodeKind {
    CapabilityCall(CapabilityCallIr),
    HelperCall(HelperCallIr),
    CollectionMap(CollectionIr),
    CollectionFilter(CollectionIr),
    CollectionSome(CollectionIr),
    CollectionEvery(CollectionIr),
    Condition(ConditionIr),
    Transform(TransformIr),
    EmitCandidates(EmitCandidatesIr),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CapabilityCallIr {
    pub capability: CapabilityName,
    pub literal_limit: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HelperCallIr {
    pub helper: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CollectionIr {
    pub input: NodeId,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConditionIr {
    pub expression_summary: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TransformIr {
    pub expression_summary: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EmitCandidatesIr {
    pub input: NodeId,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StaticCost {
    pub ast_nodes: u32,
    pub literal_bytes: usize,
    pub helper_count: u32,
    pub max_helper_call_depth: u32,
    pub max_capability_calls: u32,
    pub max_aggregate_capability_results: u32,
    pub max_collection_traversals: u32,
    pub max_output_candidates: u32,
    pub max_output_bytes: usize,
}

impl WorkflowIr {
    pub fn empty() -> Self {
        Self {
            ir_schema_version: IR_SCHEMA_V1,
            entry: 0,
            helpers: Vec::new(),
            nodes: Vec::new(),
            output: 0,
            capability_manifest: CapabilityManifest::default(),
            static_cost: StaticCost {
                ast_nodes: 0,
                literal_bytes: 0,
                helper_count: 0,
                max_helper_call_depth: 0,
                max_capability_calls: 0,
                max_aggregate_capability_results: 0,
                max_collection_traversals: 0,
                max_output_candidates: 0,
                max_output_bytes: 0,
            },
            possible_edit_kinds: BTreeSet::new(),
        }
    }
}
