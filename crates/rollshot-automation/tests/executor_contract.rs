use std::time::Duration;

use rollshot_automation::{
    ensure_compatible, validate_source, AutomationExecution, AutomationExecutor, AutomationHost,
    AutomationInput, CancellationFlag, ExecutionError, ExecutionMetrics, ExecutionPolicy,
    FakeAutomationHost, ProposalContext, ValidationLimits,
};
use rollshot_edit_proposal::{ProposalId, Provenance, ProvenanceSource};

struct EchoExecutor;

impl AutomationExecutor for EchoExecutor {
    fn execute(
        &self,
        automation: &rollshot_automation::ValidatedAutomation,
        _input: &AutomationInput,
        _proposal: &ProposalContext,
        _host: &mut dyn AutomationHost,
        _policy: &ExecutionPolicy,
        cancellation: &CancellationFlag,
    ) -> Result<AutomationExecution, ExecutionError> {
        ensure_compatible(automation)?;
        if cancellation.is_cancelled() {
            return Err(ExecutionError::Cancelled);
        }
        Ok(AutomationExecution {
            output_json: r#"{"candidates":[]}"#.into(),
            metrics: ExecutionMetrics {
                duration: Duration::ZERO,
                capability_calls: 0,
                output_bytes: 17,
                interrupted: false,
            },
        })
    }
}

#[test]
fn executor_contract_checks_compatibility_and_cancellation() {
    let automation = validate_source(
        "function main(input) { return { candidates: [] }; }",
        &ValidationLimits::default(),
    )
    .unwrap();
    let input = AutomationInput {
        image_width: 1,
        image_height: 1,
        region: None,
        annotations: Vec::new(),
        capability_handles: Default::default(),
    };
    let context = ProposalContext {
        proposal_id: ProposalId::parse("proposal-00000001-0000-4000-8000-000000000000").unwrap(),
        base_document_state_id: 0,
        provenance: Provenance {
            source: ProvenanceSource::Agent {
                run_id: "run-00000000-0000-4000-8000-000000000001".to_string(),
            },
        },
    };
    let policy = ExecutionPolicy::smart_redaction_default(
        Duration::from_secs(1),
        8 * 1024 * 1024,
        256 * 1024,
    );
    let cancellation = CancellationFlag::new();
    cancellation.cancel();
    let result = EchoExecutor.execute(
        &automation,
        &input,
        &context,
        &mut FakeAutomationHost::default(),
        &policy,
        &cancellation,
    );
    assert_eq!(result, Err(ExecutionError::Cancelled));
}
