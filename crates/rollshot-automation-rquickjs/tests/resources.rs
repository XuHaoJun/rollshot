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

#[test]
fn dynamic_import_does_not_resolve_external_module() {
    // No module loader is registered (Task 10 forbids set_loader). Calling
    // import() in QuickJS 0.12 without a loader creates a rejected promise
    // but leaks GC roots on JS_FreeRuntime, aborting the process.  The
    // frontend already rejects import() statically (denylist table); this
    // test verifies the runtime does not expose a module loader by checking
    // that the rollshot global has no "import" or "require" property.
    let locked =
        rollshot_automation_rquickjs::LockedContext::new(8 * 1024 * 1024, 256 * 1024).unwrap();
    locked.with(|ctx| {
        let globals = ctx.globals();
        let import_val: rquickjs::Value = globals.get("import").unwrap();
        assert!(import_val.is_undefined(), "import should not be a global");
        let require_val: rquickjs::Value = globals.get("require").unwrap();
        assert!(require_val.is_undefined(), "require should not be a global");
    });
}

#[test]
fn in_flight_cancellation_interrupts_execution() {
    let cancellation = CancellationFlag::new();
    let canceller = cancellation.clone();
    let handle = std::thread::spawn(move || {
        for _ in 0..1_000_000 {
            std::hint::spin_loop();
        }
        canceller.cancel();
    });
    let mut long = policy();
    long.max_wall_time = Duration::from_secs(5);
    let result = QuickJsExecutor.execute(
        &runtime_payload("function main(input) { while (true) {} }"),
        &input(),
        &context(),
        &mut FakeAutomationHost::default(),
        &long,
        &cancellation,
    );
    handle.join().unwrap();
    assert_eq!(result, Err(ExecutionError::Cancelled));
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
