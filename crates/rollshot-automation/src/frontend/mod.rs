mod normalize;
mod parse;
mod validate;

use serde::{Deserialize, Serialize};

use crate::ir::WorkflowIr;
use crate::{
    CapabilityApiVersion, IrSchemaVersion, LanguageSchemaVersion, OutputSchemaVersion,
    SourceDiagnostic, ValidationLimits, CAPABILITY_API_V1, IR_SCHEMA_V1, LANGUAGE_SCHEMA_V1,
    OUTPUT_SCHEMA_V1,
};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ValidatedAutomation {
    pub source: String,
    pub language_schema_version: LanguageSchemaVersion,
    pub ir_schema_version: IrSchemaVersion,
    pub capability_api_version: CapabilityApiVersion,
    pub output_schema_version: OutputSchemaVersion,
    pub workflow_ir: WorkflowIr,
    pub validation_summary: ValidationSummary,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ValidationSummary {
    pub source_bytes: usize,
    pub ast_nodes: u32,
    pub helper_count: u32,
    pub capability_calls: u32,
    pub max_output_candidates: u32,
}

pub fn validate_source(
    source: &str,
    limits: &ValidationLimits,
) -> Result<ValidatedAutomation, Vec<SourceDiagnostic>> {
    parse::with_program(source, |program| {
        let shape = validate::validate_shape(source, program, limits);
        let (body_diagnostics, facts) =
            validate::validate_bodies(source, program, &shape.function_names, limits);

        let mut diagnostics = shape.diagnostics;
        diagnostics.extend(body_diagnostics);

        if !diagnostics.is_empty() {
            return Err(diagnostics);
        }

        let workflow_ir = normalize::normalize(source, program, &facts, limits)?;

        let validation_summary = ValidationSummary {
            source_bytes: source.len(),
            ast_nodes: workflow_ir.static_cost.ast_nodes,
            helper_count: workflow_ir.static_cost.helper_count,
            capability_calls: workflow_ir.static_cost.max_capability_calls,
            max_output_candidates: workflow_ir.static_cost.max_output_candidates,
        };

        Ok(ValidatedAutomation {
            source: source.into(),
            language_schema_version: LANGUAGE_SCHEMA_V1,
            ir_schema_version: IR_SCHEMA_V1,
            capability_api_version: CAPABILITY_API_V1,
            output_schema_version: OUTPUT_SCHEMA_V1,
            workflow_ir,
            validation_summary,
        })
    })?
}
