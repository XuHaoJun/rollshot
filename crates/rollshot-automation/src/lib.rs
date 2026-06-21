mod diagnostic;
mod version;

pub use diagnostic::{
    DiagnosticCode, DiagnosticSeverity, RelatedDiagnostic, SourceDiagnostic, SourceSpan,
};
pub use version::{
    CapabilityApiVersion, IrSchemaVersion, LanguageSchemaVersion, OutputSchemaVersion,
    CAPABILITY_API_V1, IR_SCHEMA_V1, LANGUAGE_SCHEMA_V1, OUTPUT_SCHEMA_V1,
};
