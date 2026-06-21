use crate::{
    CapabilityApiVersion, IrSchemaVersion, LanguageSchemaVersion, OutputSchemaVersion,
    ValidatedAutomation, CAPABILITY_API_V1, IR_SCHEMA_V1, LANGUAGE_SCHEMA_V1, OUTPUT_SCHEMA_V1,
};

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CompatibilityError {
    #[error("language schema mismatch")]
    Language {
        installed: LanguageSchemaVersion,
        artifact: LanguageSchemaVersion,
    },
    #[error("IR schema mismatch")]
    Ir {
        installed: IrSchemaVersion,
        artifact: IrSchemaVersion,
    },
    #[error("capability API mismatch")]
    Capability {
        installed: CapabilityApiVersion,
        artifact: CapabilityApiVersion,
    },
    #[error("output schema mismatch")]
    Output {
        installed: OutputSchemaVersion,
        artifact: OutputSchemaVersion,
    },
}

pub fn ensure_compatible(automation: &ValidatedAutomation) -> Result<(), CompatibilityError> {
    if automation.language_schema_version != LANGUAGE_SCHEMA_V1 {
        return Err(CompatibilityError::Language {
            installed: LANGUAGE_SCHEMA_V1,
            artifact: automation.language_schema_version,
        });
    }
    if automation.ir_schema_version != IR_SCHEMA_V1 {
        return Err(CompatibilityError::Ir {
            installed: IR_SCHEMA_V1,
            artifact: automation.ir_schema_version,
        });
    }
    if automation.capability_api_version != CAPABILITY_API_V1 {
        return Err(CompatibilityError::Capability {
            installed: CAPABILITY_API_V1,
            artifact: automation.capability_api_version,
        });
    }
    if automation.output_schema_version != OUTPUT_SCHEMA_V1 {
        return Err(CompatibilityError::Output {
            installed: OUTPUT_SCHEMA_V1,
            artifact: automation.output_schema_version,
        });
    }
    Ok(())
}
