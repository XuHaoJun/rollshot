use rollshot_agent::runtime::RunBudget;
use rollshot_automation::{
    execute_to_proposal, AutomationInput, CancellationFlag, ExecutionPolicy, ProposalContext,
};
use rollshot_automation_rquickjs::QuickJsExecutor;
use rollshot_edit_proposal::{EditProposal, ProposalId, Provenance, ProvenanceSource};
use rollshot_preset::AutomationRevision;
use rollshot_vision::VisualIndex;

use super::state::WorkbenchError;

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

/// Run a preset's active `ValidatedAutomation` against the given image
/// (no LLM, no upload). Builds `VisualIndex`, prepares a fresh
/// `RealAutomationHost`, and runs the automation via `execute_to_proposal`.
/// Returns the dry-run `EditProposal`.
pub fn run_existing_preset(
    image: &image::RgbaImage,
    revision: &AutomationRevision,
    policy: &ExecutionPolicy,
) -> Result<EditProposal, WorkbenchError> {
    let (w, h) = image.dimensions();
    let _index = VisualIndex::build(image.clone())
        .map_err(|e| WorkbenchError::VisionPrepare {
            message: format!("VisualIndex: {e}"),
        })?;
    let mut host = rollshot_vision::RealAutomationHost::new();
    let executor = QuickJsExecutor;
    let cancellation = CancellationFlag::default();
    let input = AutomationInput {
        image_width: w,
        image_height: h,
        region: None,
        annotations: vec![],
        capability_handles: Default::default(),
    };
    let ctx = ProposalContext {
        proposal_id: ProposalId(1),
        base_document_state_id: 0,
        provenance: Provenance {
            source: ProvenanceSource::Manual,
        },
    };
    let (proposal, _metrics) = execute_to_proposal(
        &executor,
        &revision.artifact,
        &input,
        &ctx,
        &mut host,
        policy,
        &cancellation,
    )
    .map_err(|_| WorkbenchError::RuntimeFailure)?;
    Ok(proposal)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rollshot_automation::{
        execute_to_proposal, AutomationInput, CancellationFlag, ExecutionPolicy,
        FakeAutomationHost, ProposalContext,
    };
    use rollshot_automation_rquickjs::QuickJsExecutor;
    use rollshot_edit_proposal::{ProposalId, Provenance, ProvenanceSource};

    fn test_image() -> image::RgbaImage {
        image::RgbaImage::from_fn(64, 64, |_, _| image::Rgba([200, 200, 200, 255]))
    }

    #[test]
    fn execute_dry_run_with_empty_main_returns_zero_candidates() {
        let source = r#"function main(input) { return { candidates: [] }; }"#;
        let limits = rollshot_automation::ValidationLimits::default();
        let validated = rollshot_automation::validate_source(source, &limits).unwrap();
        let policy = ExecutionPolicy::smart_redaction_default(
            std::time::Duration::from_secs(10),
            100_000_000,
            8_000_000,
        );
        let cancellation = CancellationFlag::default();
        let executor = QuickJsExecutor;
        let input = AutomationInput {
            image_width: 64,
            image_height: 64,
            region: None,
            annotations: vec![],
            capability_handles: Default::default(),
        };
        let ctx = ProposalContext {
            proposal_id: ProposalId(1),
            base_document_state_id: 0,
            provenance: Provenance {
                source: ProvenanceSource::Manual,
            },
        };
        let mut host = FakeAutomationHost::default();
        let result = execute_to_proposal(
            &executor,
            &validated,
            &input,
            &ctx,
            &mut host,
            &policy,
            &cancellation,
        );
        let (proposal, _metrics) = result.unwrap();
        assert_eq!(proposal.candidates.len(), 0);
    }

    #[test]
    fn run_existing_preset_rejects_empty_image() {
        let empty = image::RgbaImage::new(0, 0);
        let revision = make_empty_revision();
        let policy = ExecutionPolicy::smart_redaction_default(
            std::time::Duration::from_secs(10),
            100_000_000,
            8_000_000,
        );
        let result = run_existing_preset(&empty, &revision, &policy);
        assert!(matches!(result, Err(WorkbenchError::VisionPrepare { .. })));
    }

    fn make_empty_revision() -> AutomationRevision {
        use rollshot_preset::*;
        let source = r#"function main(input) { return { candidates: [] }; }"#;
        let limits = rollshot_automation::ValidationLimits::default();
        let validated = rollshot_automation::validate_source(source, &limits).unwrap();
        AutomationRevision {
            store_schema_version: STORE_SCHEMA_VERSION,
            id: RevisionId("rev-1".into()),
            preset_id: PresetId("test".into()),
            parent_id: None,
            created_at: "2026-01-01T00:00:00Z".into(),
            provenance: RevisionProvenance {
                origin: RevisionOrigin::Manual,
                note: None,
                source_run_ref: None,
            },
            artifact: validated,
        }
    }
}
