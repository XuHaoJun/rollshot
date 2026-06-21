use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::time::Duration;

use crate::{
    AutomationHost, CapabilityApiVersion, CapabilityError, ExecutionPolicy, IrSchemaVersion,
    LanguageSchemaVersion, OutputError, OutputSchemaVersion, ProposalContext, ValidatedAutomation,
    CAPABILITY_API_V1, IR_SCHEMA_V1, LANGUAGE_SCHEMA_V1, OUTPUT_SCHEMA_V1,
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

#[derive(Debug, Clone, Default)]
pub struct CancellationFlag(Arc<AtomicBool>);

impl CancellationFlag {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn cancel(&self) {
        self.0.store(true, Ordering::SeqCst);
    }

    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutionMetrics {
    pub duration: Duration,
    pub capability_calls: u32,
    pub output_bytes: usize,
    pub interrupted: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AutomationExecution {
    pub output_json: String,
    pub metrics: ExecutionMetrics,
}

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum SandboxError {
    #[error("sandbox initialization failed: {code}")]
    Initialization { code: &'static str },
    #[error("sandbox memory limit")]
    MemoryLimit,
    #[error("sandbox stack limit")]
    StackLimit,
    #[error("sandbox timeout")]
    Timeout,
    #[error("sandbox interrupted")]
    Interrupted,
    #[error("sandbox evaluation failed: {code}")]
    Evaluation { code: &'static str },
}

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum ExecutionError {
    #[error(transparent)]
    Compatibility(#[from] CompatibilityError),
    #[error(transparent)]
    Sandbox(#[from] SandboxError),
    #[error(transparent)]
    Capability(#[from] CapabilityError),
    #[error(transparent)]
    Output(#[from] OutputError),
    #[error("execution cancelled")]
    Cancelled,
}

pub trait AutomationExecutor {
    fn execute(
        &self,
        automation: &ValidatedAutomation,
        input: &crate::AutomationInput,
        proposal: &ProposalContext,
        host: &mut dyn AutomationHost,
        policy: &ExecutionPolicy,
        cancellation: &CancellationFlag,
    ) -> Result<AutomationExecution, ExecutionError>;
}

pub fn execute_to_proposal(
    executor: &dyn AutomationExecutor,
    automation: &ValidatedAutomation,
    input: &crate::AutomationInput,
    proposal: &ProposalContext,
    host: &mut dyn AutomationHost,
    policy: &ExecutionPolicy,
    cancellation: &CancellationFlag,
) -> Result<(rollshot_edit_proposal::EditProposal, ExecutionMetrics), ExecutionError> {
    let execution = executor.execute(automation, input, proposal, host, policy, cancellation)?;
    let edit_proposal = crate::decode_proposal(
        &execution.output_json,
        (input.image_width, input.image_height),
        proposal,
        policy,
    )?;
    Ok((edit_proposal, execution.metrics))
}
