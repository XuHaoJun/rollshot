use std::time::Duration;

use rollshot_automation::{
    validate_source, AutomationExecutor, AutomationInput, CancellationFlag, ExecutionError,
    ExecutionPolicy, FakeAutomationHost, ProposalContext, ValidationLimits,
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
        proposal_id: ProposalId::parse("proposal-00000001-0000-4000-8000-000000000000").unwrap(),
        base_document_state_id: 0,
        provenance: Provenance {
            source: ProvenanceSource::Agent { run_id: "run-00000000-0000-4000-8000-000000000001".to_string() },
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
fn modified_validated_source_is_rejected_before_runtime_creation() {
    let automation =
        runtime_payload("function main(input) { while (true) {} return { candidates: [] }; }");
    let result = QuickJsExecutor.execute(
        &automation,
        &input(),
        &context(),
        &mut FakeAutomationHost::default(),
        &policy(),
        &CancellationFlag::new(),
    );
    assert!(matches!(result, Err(ExecutionError::Compatibility(_))));
}

#[test]
fn dynamic_import_does_not_resolve_external_module() {
    let locked =
        rollshot_automation_rquickjs::LockedContext::new(8 * 1024 * 1024, 256 * 1024).unwrap();
    locked.with(|ctx| {
        let globals = ctx.globals();
        let import_value: rquickjs::Value = globals.get("import").unwrap();
        let require_value: rquickjs::Value = globals.get("require").unwrap();
        assert!(import_value.is_undefined());
        assert!(require_value.is_undefined());
    });
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
            quad: [
                ImagePoint { x: 0.0, y: 0.0 },
                ImagePoint { x: 1.0, y: 0.0 },
                ImagePoint { x: 1.0, y: 1.0 },
                ImagePoint { x: 0.0, y: 1.0 },
            ],
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
    let automation = validate_source(
        "function main(input) { return { candidates: [], padding: 'xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx' }; }",
        &ValidationLimits::default(),
    )
    .unwrap();
    let result = QuickJsExecutor.execute(
        &automation,
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
fn global_capability_call_limit_is_enforced() {
    use rollshot_automation::CapabilityError;

    let source = r#"
function main(input) {
  const a = rollshot.ocr({ region: { kind: "full" }, limit: 1 });
  const b = rollshot.ocr({ region: { kind: "full" }, limit: 1 });
  const c = rollshot.ocr({ region: { kind: "full" }, limit: 1 });
  return { candidates: [] };
}
"#;
    let automation = validate_source(source, &ValidationLimits::default()).unwrap();
    let mut policy = policy();
    policy.max_capability_calls = 2;
    let result = QuickJsExecutor.execute(
        &automation,
        &input(),
        &context(),
        &mut FakeAutomationHost::default(),
        &policy,
        &CancellationFlag::new(),
    );
    assert_eq!(
        result,
        Err(ExecutionError::Capability(CapabilityError::LimitExceeded))
    );
}

#[test]
fn per_capability_call_limit_is_enforced() {
    use rollshot_automation::CapabilityError;

    let source = r#"
function main(input) {
  const a = rollshot.ocr({ region: { kind: "full" }, limit: 1 });
  const b = rollshot.ocr({ region: { kind: "full" }, limit: 1 });
  return { candidates: [] };
}
"#;
    let automation = validate_source(source, &ValidationLimits::default()).unwrap();
    let mut policy = policy();
    policy.max_capability_calls = 16;
    policy
        .max_calls_by_capability
        .insert(rollshot_automation::CapabilityName::Ocr, 1);
    let result = QuickJsExecutor.execute(
        &automation,
        &input(),
        &context(),
        &mut FakeAutomationHost::default(),
        &policy,
        &CancellationFlag::new(),
    );
    assert_eq!(
        result,
        Err(ExecutionError::Capability(CapabilityError::LimitExceeded))
    );
}
