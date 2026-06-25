use rollshot_agent::runtime::RunBudget;

/// Finite budget for Smart Redaction runs. `RunBudget::unlimited()` is the
/// only constructor in rollshot-agent (§10.4); the workbench owns this one.
pub fn smart_redaction_budget() -> RunBudget {
    RunBudget {
        wall_time: std::time::Duration::from_secs(30),
        model_calls: 10,
        input_tokens: 20_000,
        output_tokens: 10_000,
        cost: 0.50,
        tool_calls: 30,
        per_tool_calls: 10,
        argument_bytes: 256 * 1024,
        result_bytes: 256 * 1024,
        source_bytes: 100 * 1024,
        attachments: 8,
        validation_attempts: 10,
        dry_run_attempts: 5,
        capability_calls: 16,
        candidate_count: 1000,
        affected_area: 1,
    }
}
