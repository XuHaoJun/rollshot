use std::time::Duration;

use rollshot_automation::{
    validate_source, AutomationExecutor, AutomationInput, CancellationFlag, ExecutionError,
    ExecutionPolicy, FakeAutomationHost, ProposalContext, SandboxError, ValidationLimits,
};
use rollshot_automation_rquickjs::QuickJsExecutor;
use rollshot_edit_proposal::{ProposalId, Provenance, ProvenanceSource};

fn input() -> AutomationInput {
    AutomationInput {
        image_width: 10,
        image_height: 10,
        region: None,
        annotations: Vec::new(),
        capability_handles: Default::default(),
    }
}

fn context() -> ProposalContext {
    ProposalContext {
        proposal_id: ProposalId(1),
        base_document_state_id: 0,
        provenance: Provenance {
            source: ProvenanceSource::Agent { run_id: 1 },
        },
    }
}

fn policy() -> ExecutionPolicy {
    ExecutionPolicy::smart_redaction_default(Duration::from_millis(25), 4 * 1024 * 1024, 128 * 1024)
}

#[test]
fn pre_cancelled_execution_never_runs() {
    let automation = validate_source(
        "function main(input) { return { candidates: [] }; }",
        &ValidationLimits::default(),
    )
    .unwrap();
    let cancellation = CancellationFlag::new();
    cancellation.cancel();
    let result = QuickJsExecutor.execute(
        &automation,
        &input(),
        &context(),
        &mut FakeAutomationHost::default(),
        &policy(),
        &cancellation,
    );
    assert_eq!(result, Err(ExecutionError::Cancelled));
}

fn runtime_payload(source: &str) -> rollshot_automation::ValidatedAutomation {
    let mut automation = validate_source(
        "function main(input) { return { candidates: [] }; }",
        &ValidationLimits::default(),
    )
    .unwrap();
    automation.source = source.into();
    automation
}

#[test]
fn interrupt_stops_infinite_runtime_payload() {
    let result = QuickJsExecutor.execute(
        &runtime_payload("function main(input) { while (true) {} }"),
        &input(),
        &context(),
        &mut FakeAutomationHost::default(),
        &policy(),
        &CancellationFlag::new(),
    );
    assert!(matches!(
        result,
        Err(ExecutionError::Sandbox(SandboxError::Timeout))
            | Err(ExecutionError::Sandbox(SandboxError::Interrupted))
    ));
}

#[test]
fn memory_limit_stops_runtime_allocation() {
    let mut limits = policy();
    limits.max_wall_time = Duration::from_secs(1);
    limits.max_memory_bytes = 1024 * 1024;
    let result = QuickJsExecutor.execute(
        &runtime_payload(
            "function main(input) { const a = []; while (true) { a.push(new Array(1000).fill(1)); } }",
        ),
        &input(),
        &context(),
        &mut FakeAutomationHost::default(),
        &limits,
        &CancellationFlag::new(),
    );
    assert_eq!(
        result,
        Err(ExecutionError::Sandbox(SandboxError::MemoryLimit))
    );
}

#[test]
fn stack_limit_stops_runtime_recursion() {
    let result = QuickJsExecutor.execute(
        &runtime_payload(
            "function recurse() { return recurse(); } function main(input) { return recurse(); }",
        ),
        &input(),
        &context(),
        &mut FakeAutomationHost::default(),
        &policy(),
        &CancellationFlag::new(),
    );
    assert_eq!(
        result,
        Err(ExecutionError::Sandbox(SandboxError::StackLimit))
    );
}

#[test]
fn host_allocation_limit_rejects_large_capability_result() {
    use rollshot_automation::OcrMatch;
    use rollshot_image_document::{ImagePoint, ImageRect};

    let source = r#"
function main(input) {
  rollshot.ocr({ region: { kind: "full" }, limit: 1 });
  return { candidates: [] };
}
"#;
    let automation = validate_source(source, &ValidationLimits::default()).unwrap();
    let mut host = FakeAutomationHost {
        ocr_results: vec![OcrMatch {
            bounds: ImageRect::from_corners(ImagePoint::new(0.0, 0.0), ImagePoint::new(1.0, 1.0)),
            text: "x".repeat(4_096),
            confidence: 1.0,
        }],
        ..Default::default()
    };
    let mut limits = policy();
    limits.max_host_allocation_bytes = 128;
    assert_eq!(
        QuickJsExecutor.execute(
            &automation,
            &input(),
            &context(),
            &mut host,
            &limits,
            &CancellationFlag::new(),
        ),
        Err(ExecutionError::Capability(
            rollshot_automation::CapabilityError::LimitExceeded,
        ))
    );
}

#[test]
fn output_byte_limit_is_enforced_before_decoding() {
    let mut limits = policy();
    limits.max_output_bytes = 32;
    let result = QuickJsExecutor.execute(
        &runtime_payload(
            "function main(input) { return { candidates: [], padding: 'x'.repeat(1024) }; }",
        ),
        &input(),
        &context(),
        &mut FakeAutomationHost::default(),
        &limits,
        &CancellationFlag::new(),
    );
    assert_eq!(
        result,
        Err(ExecutionError::Output(
            rollshot_automation::OutputError::TooLarge,
        ))
    );
}

#[test]
fn fresh_execution_does_not_observe_prior_global_state() {
    let executor = QuickJsExecutor;
    let first = executor.execute(
        &runtime_payload(
            "var __rollshot_marker = 1; function main(input) { return { candidates: [] }; }",
        ),
        &input(),
        &context(),
        &mut FakeAutomationHost::default(),
        &policy(),
        &CancellationFlag::new(),
    );
    assert!(first.is_ok());

    let second = executor
        .execute(
            &runtime_payload(
                "function main(input) { return { candidates: typeof __rollshot_marker === 'undefined' ? [] : [{ kind: 'delete', annotationId: '1', confidence: 1, label: 'leak' }] }; }",
            ),
            &input(),
            &context(),
            &mut FakeAutomationHost::default(),
            &policy(),
            &CancellationFlag::new(),
        )
        .unwrap();
    assert_eq!(second.output_json, r#"{"candidates":[]}"#);
}
