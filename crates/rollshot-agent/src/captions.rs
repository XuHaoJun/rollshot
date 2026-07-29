//! Caption-run budget. The provider-neutral half of the Action Guide caption
//! flow; the guide model and draft types stay in `rollshot-action`, which this
//! crate must not depend on.

use crate::runtime::RunBudget;

/// Tight caption budget. Wall time and output tokens match the pre-migration
/// timeout and `max_tokens` exactly, so observable timing does not change.
pub fn caption_run_budget() -> RunBudget {
    RunBudget {
        wall_time: std::time::Duration::from_secs(30),
        model_calls: 2,
        input_tokens: 32_000,
        output_tokens: 1_200,
        cost: f64::MAX,
        tool_calls: 1,
        per_tool_calls: 1,
        argument_bytes: 4_096,
        result_bytes: 4_096,
        source_bytes: 0,
        attachments: 0,
        validation_attempts: 0,
        dry_run_attempts: 0,
        capability_calls: 0,
        candidate_count: 0,
        affected_area: 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn caption_budget_sends_no_attachments_and_keeps_the_thirty_second_bound() {
        let budget = caption_run_budget();

        assert_eq!(budget.attachments, 0);
        assert_eq!(budget.wall_time, std::time::Duration::from_secs(30));
        assert_eq!(budget.model_calls, 2);
        assert_eq!(budget.tool_calls, 1);
        assert_eq!(budget.output_tokens, 1_200);
        assert_eq!(budget.dry_run_attempts, 0);
        assert_eq!(budget.candidate_count, 0);
    }
}
