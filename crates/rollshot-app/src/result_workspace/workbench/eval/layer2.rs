use rollshot_automation::{
    execute_to_proposal, validate_source, AutomationInput, CancellationFlag, ExecutionPolicy,
    ProposalContext, ProposedEditKind, Region, ValidationLimits,
};
use rollshot_automation_rquickjs::QuickJsExecutor;
use rollshot_edit_proposal::{ProposalId, ProposedEdit, Provenance, ProvenanceSource};
use rollshot_image_document::ImageRect;

use crate::result_workspace::workbench::run::{prepare_vision_context, ProductCapabilityBundle};

pub(crate) fn run_golden_source(
    image: &image::RgbaImage,
    golden_js: &str,
) -> Result<Vec<ImageRect>, String> {
    let (w, h) = image.dimensions();
    let automation = validate_source(golden_js, &ValidationLimits::default())
        .map_err(|e| format!("validate: {e:?}"))?;
    let vision = prepare_vision_context(image, &ProductCapabilityBundle::empty())
        .map_err(|e| format!("prepare: {e:?}"))?;

    let input = AutomationInput {
        image_width: w,
        image_height: h,
        region: Some(Region::Full),
        annotations: Vec::new(),
        capability_handles: Default::default(),
    };
    let ctx = ProposalContext {
        proposal_id: ProposalId::parse("proposal-00000001-0000-4000-8000-000000000000").unwrap(),
        base_document_state_id: 0,
        provenance: Provenance {
            source: ProvenanceSource::Manual,
        },
    };
    let mut policy = ExecutionPolicy::smart_redaction_default(
        std::time::Duration::from_secs(5),
        16 * 1024 * 1024,
        256 * 1024,
    );
    policy
        .allowed_edit_kinds
        .insert(ProposedEditKind::AddRedaction);

    let cancellation = CancellationFlag::new();
    let mut host_guard = vision.host.lock().unwrap();
    let (proposal, _metrics) = execute_to_proposal(
        &QuickJsExecutor,
        &automation,
        &input,
        &ctx,
        &mut *host_guard,
        &policy,
        &cancellation,
    )
    .map_err(|e| format!("execute: {e:?}"))?;

    Ok(proposal
        .candidates
        .into_iter()
        .filter_map(|c| match c.edit {
            ProposedEdit::AddRedaction { bounds } => Some(bounds),
            _ => None,
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::result_workspace::workbench::eval::render::render_url_bar;

    /// A region-feature golden that redacts the top strip where the URL bar
    /// lives. Uses only region features so it runs without the `ocr` feature.
    const TOP_STRIP_GOLDEN: &str = r#"
function main(input) {
  return {
    candidates: [{
      kind: 'addRedaction',
      bounds: { x: 120, y: 14, width: 600, height: 28 },
      confidence: 0.9,
      label: 'url'
    }]
  };
}
"#;

    #[test]
    fn golden_source_produces_candidate_over_url_bar() {
        let f = render_url_bar();
        let cands = run_golden_source(&f.image, TOP_STRIP_GOLDEN).expect("layer2 runs");
        assert_eq!(cands.len(), 1);
        let c = &cands[0];
        assert!((c.x - 120.0).abs() < 1.0 && (c.width - 600.0).abs() < 1.0);
    }
}
