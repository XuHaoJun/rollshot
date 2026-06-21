use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Instant;

use rollshot_automation::{
    ensure_compatible, AutomationExecution, AutomationExecutor, AutomationHost, AutomationInput,
    CancellationFlag, ExecutionError, ExecutionMetrics, ExecutionPolicy, ProposalContext,
    SandboxError, ValidatedAutomation,
};
use rquickjs::{CaughtError, Function};

use crate::bridge::{install_input, install_rollshot, BridgeGuard};
use crate::lockdown::LockedContext;

fn classify_rquickjs_error(err: rquickjs::Error, ctx: &rquickjs::Ctx<'_>) -> SandboxError {
    match err {
        rquickjs::Error::Allocation => SandboxError::MemoryLimit,
        rquickjs::Error::Exception => {
            let caught = CaughtError::from_error(ctx, rquickjs::Error::Exception);
            match caught {
                CaughtError::Exception(ex) => {
                    let msg = ex.message().unwrap_or_default();
                    if msg.contains("stack") || msg.contains("RangeError") {
                        SandboxError::StackLimit
                    } else if msg.contains("out of memory") {
                        SandboxError::MemoryLimit
                    } else {
                        SandboxError::Evaluation { code: "exception" }
                    }
                }
                _ => SandboxError::Evaluation { code: "exception" },
            }
        }
        _ => SandboxError::Evaluation { code: "runtime" },
    }
}

impl AutomationExecutor for crate::QuickJsExecutor {
    fn execute(
        &self,
        automation: &ValidatedAutomation,
        input: &AutomationInput,
        _proposal: &ProposalContext,
        host: &mut dyn AutomationHost,
        policy: &ExecutionPolicy,
        cancellation: &CancellationFlag,
    ) -> Result<AutomationExecution, ExecutionError> {
        self.execute_inner(automation, input, host, policy, cancellation, true)
    }
}

impl crate::QuickJsExecutor {
    fn execute_inner(
        &self,
        automation: &ValidatedAutomation,
        input: &AutomationInput,
        host: &mut dyn AutomationHost,
        policy: &ExecutionPolicy,
        cancellation: &CancellationFlag,
        verify_automation: bool,
    ) -> Result<AutomationExecution, ExecutionError> {
        if verify_automation {
            ensure_compatible(automation).map_err(ExecutionError::Compatibility)?;
        }
        if cancellation.is_cancelled() {
            return Err(ExecutionError::Cancelled);
        }

        let started = Instant::now();
        let deadline = started + policy.max_wall_time;
        let interrupted = Arc::new(AtomicBool::new(false));
        let interrupted_for_handler = Arc::clone(&interrupted);
        let cancellation_for_handler = cancellation.clone();

        let locked = LockedContext::new(policy.max_memory_bytes, policy.max_stack_bytes)?;
        locked
            .runtime()
            .set_interrupt_handler(Some(Box::new(move || {
                let should_stop =
                    cancellation_for_handler.is_cancelled() || Instant::now() >= deadline;
                if should_stop {
                    interrupted_for_handler.store(true, Ordering::SeqCst);
                }
                should_stop
            })));

        let mut guard = BridgeGuard::new(host, policy, automation);
        guard.register();
        let bridge_id = guard.id();

        let output_json = locked.with(|ctx| -> Result<String, SandboxError> {
            let input_value =
                install_input(&ctx, input).map_err(|e| classify_rquickjs_error(e, &ctx))?;
            install_rollshot(&ctx, bridge_id).map_err(|e| classify_rquickjs_error(e, &ctx))?;
            ctx.eval::<(), _>(automation.source.as_bytes())
                .map_err(|e| classify_rquickjs_error(e, &ctx))?;
            let main: Function =
                ctx.globals()
                    .get("main")
                    .map_err(|_| SandboxError::Evaluation {
                        code: "missing_main",
                    })?;
            let value: rquickjs::Value = main
                .call((input_value,))
                .map_err(|e| classify_rquickjs_error(e, &ctx))?;
            let stringified = ctx
                .json_stringify(value)
                .map_err(|e| classify_rquickjs_error(e, &ctx))?
                .ok_or(SandboxError::Evaluation {
                    code: "undefined_output",
                })?;
            stringified
                .to_string()
                .map_err(|_| SandboxError::Evaluation {
                    code: "utf8_output",
                })
        });

        let duration = started.elapsed();
        let pending_error = guard.inner_mut().pending_error.take();
        let capability_calls = guard.inner().capability_calls;

        // Classification ORDER matters — the tests assert exact variants.
        // 1. cancellation flag set          → Cancelled
        // 2. bridge stored a typed error    → Capability(..)
        // 3. interrupt fired + deadline hit  → Timeout
        // 4. sandbox error (MemoryLimit, StackLimit, etc.)  → pass through
        // 5. success                         → check output byte ceiling, then Ok
        if cancellation.is_cancelled() {
            return Err(ExecutionError::Cancelled);
        }
        if let Some(error) = pending_error {
            return Err(ExecutionError::Capability(error));
        }
        match output_json {
            Err(SandboxError::Evaluation { .. })
                if interrupted.load(Ordering::SeqCst) && duration >= policy.max_wall_time =>
            {
                Err(ExecutionError::Sandbox(SandboxError::Timeout))
            }
            Err(e) => Err(ExecutionError::Sandbox(e)),
            Ok(json) => {
                if json.len() > policy.max_output_bytes {
                    return Err(ExecutionError::Output(
                        rollshot_automation::OutputError::TooLarge,
                    ));
                }
                tracing::debug!(
                    target: "rollshot::automation::executor",
                    duration_ms = duration.as_millis() as u64,
                    capability_calls,
                    output_bytes = json.len(),
                    interrupted = interrupted.load(Ordering::SeqCst),
                    "automation execution completed"
                );
                Ok(AutomationExecution {
                    metrics: ExecutionMetrics {
                        duration,
                        capability_calls,
                        output_bytes: json.len(),
                        interrupted: interrupted.load(Ordering::SeqCst),
                    },
                    output_json: json,
                })
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use rollshot_automation::{
        validate_source, AutomationInput, CancellationFlag, ExecutionError, ExecutionPolicy,
        FakeAutomationHost, SandboxError, ValidationLimits,
    };

    use crate::QuickJsExecutor;

    fn input() -> AutomationInput {
        AutomationInput {
            image_width: 10,
            image_height: 10,
            region: None,
            annotations: Vec::new(),
            capability_handles: Default::default(),
        }
    }

    fn policy() -> ExecutionPolicy {
        ExecutionPolicy::smart_redaction_default(
            Duration::from_millis(25),
            4 * 1024 * 1024,
            128 * 1024,
        )
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

    fn execute_unchecked(
        source: &str,
        policy: &ExecutionPolicy,
        cancellation: &CancellationFlag,
    ) -> Result<rollshot_automation::AutomationExecution, ExecutionError> {
        QuickJsExecutor.execute_inner(
            &runtime_payload(source),
            &input(),
            &mut FakeAutomationHost::default(),
            policy,
            cancellation,
            false,
        )
    }

    #[test]
    fn interrupt_stops_infinite_runtime_payload() {
        let result = execute_unchecked(
            "function main(input) { while (true) {} }",
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
        assert_eq!(
            execute_unchecked(
                "function main(input) { const a = []; while (true) { a.push(new Array(1000).fill(1)); } }",
                &limits,
                &CancellationFlag::new(),
            ),
            Err(ExecutionError::Sandbox(SandboxError::MemoryLimit))
        );
    }

    #[test]
    fn stack_limit_stops_runtime_recursion() {
        assert_eq!(
            execute_unchecked(
                "function recurse() { return recurse(); } function main(input) { return recurse(); }",
                &policy(),
                &CancellationFlag::new(),
            ),
            Err(ExecutionError::Sandbox(SandboxError::StackLimit))
        );
    }

    #[test]
    fn fresh_execution_does_not_observe_prior_global_state() {
        let executor = QuickJsExecutor;
        let cancellation = CancellationFlag::new();
        let first = executor.execute_inner(
            &runtime_payload(
                "var __rollshot_marker = 1; function main(input) { return { candidates: [] }; }",
            ),
            &input(),
            &mut FakeAutomationHost::default(),
            &policy(),
            &cancellation,
            false,
        );
        assert!(first.is_ok());

        let second = executor
            .execute_inner(
                &runtime_payload(
                    "function main(input) { return { candidates: typeof __rollshot_marker === 'undefined' ? [] : [{ kind: 'delete', annotationId: '1', confidence: 1, label: 'leak' }] }; }",
                ),
                &input(),
                &mut FakeAutomationHost::default(),
                &policy(),
                &cancellation,
                false,
            )
            .unwrap();
        assert_eq!(second.output_json, r#"{"candidates":[]}"#);
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
        let mut limits = policy();
        limits.max_wall_time = Duration::from_secs(5);
        let result = execute_unchecked(
            "function main(input) { while (true) {} }",
            &limits,
            &cancellation,
        );
        handle.join().unwrap();
        assert_eq!(result, Err(ExecutionError::Cancelled));
    }

    #[test]
    fn bridge_rejects_query_limit_above_validated_manifest() {
        let mut automation = validate_source(
            "function main(input) { rollshot.ocr({ region: { kind: 'full' }, limit: 1 }); return { candidates: [] }; }",
            &ValidationLimits::default(),
        )
        .unwrap();
        automation.source = "function main(input) { rollshot.ocr({ region: { kind: 'full' }, limit: 2 }); return { candidates: [] }; }".into();
        assert_eq!(
            QuickJsExecutor.execute_inner(
                &automation,
                &input(),
                &mut FakeAutomationHost::default(),
                &policy(),
                &CancellationFlag::new(),
                false,
            ),
            Err(ExecutionError::Capability(
                rollshot_automation::CapabilityError::InvalidInput {
                    code: "limit_exceeds_manifest"
                }
            ))
        );
    }
}
