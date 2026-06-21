mod parse;
mod validate;

use serde::{Deserialize, Serialize};

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
}

pub fn validate_source(
    source: &str,
    limits: &ValidationLimits,
) -> Result<ValidatedAutomation, Vec<SourceDiagnostic>> {
    parse::with_program(source, |program| {
        let result = validate::validate_shape(source, program, limits);
        if result.diagnostics.is_empty() {
            Ok(ValidatedAutomation {
                source: source.into(),
                language_schema_version: LANGUAGE_SCHEMA_V1,
                ir_schema_version: IR_SCHEMA_V1,
                capability_api_version: CAPABILITY_API_V1,
                output_schema_version: OUTPUT_SCHEMA_V1,
            })
        } else {
            Err(result.diagnostics)
        }
    })?
}
