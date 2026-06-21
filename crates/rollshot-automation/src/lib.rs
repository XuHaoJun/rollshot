mod capability;
mod diagnostic;
mod frontend;
mod host;
mod input;
pub mod ir;
mod policy;
mod version;

pub use capability::*;
pub use diagnostic::{
    DiagnosticCode, DiagnosticSeverity, RelatedDiagnostic, SourceDiagnostic, SourceSpan,
};
pub use frontend::{validate_source, ValidatedAutomation, ValidationSummary};
pub use host::{AutomationHost, CapabilityError, FakeAutomationHost};
pub use input::{AnnotationDescriptor, AutomationInput};
pub use ir::{CapabilityCallIr, CollectionIr, EmitCandidatesIr, IrNodeKind, WorkflowIr};
pub use policy::{ExecutionPolicy, ProposalContext, ProposedEditKind, ValidationLimits};
pub use version::{
    CapabilityApiVersion, IrSchemaVersion, LanguageSchemaVersion, OutputSchemaVersion,
    CAPABILITY_API_V1, IR_SCHEMA_V1, LANGUAGE_SCHEMA_V1, OUTPUT_SCHEMA_V1,
};
