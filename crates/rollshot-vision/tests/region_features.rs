use std::time::Duration;

use rollshot_automation::{
    execute_to_proposal, validate_source, AutomationInput, CancellationFlag, ExecutionPolicy,
    ProposalContext, ProposedEditKind, Region, RegionFeaturesQuery, ValidationLimits,
};
use rollshot_automation_rquickjs::QuickJsExecutor;
use rollshot_edit_proposal::{
    EditProposal, ProposalId, ProposedEdit, Provenance, ProvenanceSource,
};
use rollshot_image_document::ImageRect;
use rollshot_vision::{RealAutomationHost, VisualIndex};

const TOP_BAR_JS: &str = include_str!("fixtures/region_features_top_bar.js");

const STRIP_HEIGHT: u32 = 12;

/// Scene with a flat top strip (rows 0..12) and a noisy body below.
fn scene(size: u32, flat_top: bool) -> image::RgbaImage {
    image::RgbaImage::from_fn(size, size, |x, y| {
        if y < STRIP_HEIGHT && flat_top {
            image::Rgba([200, 200, 200, 255])
        } else {
            let v = if (x + y) % 2 == 0 { 0 } else { 255 };
            image::Rgba([v, v, v, 255])
        }
    })
}

fn run(scene: image::RgbaImage) -> EditProposal {
    let (w, h) = scene.dimensions();
    let automation = validate_source(TOP_BAR_JS, &ValidationLimits::default()).unwrap();
    let input = AutomationInput {
        image_width: w,
        image_height: h,
        region: Some(Region::Full),
        annotations: Vec::new(),
        capability_handles: std::collections::BTreeMap::new(),
    };
    let proposal_ctx = ProposalContext {
        proposal_id: ProposalId::parse("proposal-00000001-0000-4000-8000-000000000000").unwrap(),
        base_document_state_id: 0,
        provenance: Provenance {
            source: ProvenanceSource::Agent {
                run_id: "run-00000000-0000-4000-8000-000000000001".to_string(),
            },
        },
    };
    let mut policy = ExecutionPolicy::smart_redaction_default(
        Duration::from_secs(2),
        16 * 1024 * 1024,
        256 * 1024,
    );
    policy
        .allowed_edit_kinds
        .insert(ProposedEditKind::AddRedaction);

    let index = VisualIndex::build(scene).unwrap();
    // Prepare the SAME canonical rect the detector will query (dynamic width).
    let query = RegionFeaturesQuery {
        region: Region::Rect {
            bounds: ImageRect {
                x: 0.0,
                y: 0.0,
                width: w as f32,
                height: STRIP_HEIGHT as f32,
            },
        },
        limit: 1,
    };
    let mut host = RealAutomationHost::new();
    host.prepare_region_features(&index, &query).unwrap();

    let (proposal, _metrics) = execute_to_proposal(
        &QuickJsExecutor,
        &automation,
        &input,
        &proposal_ctx,
        &mut host,
        &policy,
        &CancellationFlag::new(),
    )
    .unwrap();
    proposal
}

#[test]
fn flat_top_strip_produces_one_candidate() {
    let proposal = run(scene(60, true));
    assert_eq!(proposal.candidates.len(), 1);
    match &proposal.candidates[0].edit {
        ProposedEdit::AddRedaction { bounds } => {
            assert_eq!(
                *bounds,
                ImageRect {
                    x: 0.0,
                    y: 0.0,
                    width: 60.0,
                    height: STRIP_HEIGHT as f32
                }
            );
        }
        other => panic!("expected AddRedaction, got {other:?}"),
    }
    assert_eq!(proposal.candidates[0].label, "top-bar-region");
}

#[test]
fn noisy_top_strip_produces_no_candidates() {
    // No flat strip -> high edge density -> filter drops it.
    let proposal = run(scene(60, false));
    assert_eq!(proposal.candidates.len(), 0);
}

#[test]
fn region_features_detection_is_deterministic() {
    let a = run(scene(60, true));
    let b = run(scene(60, true));
    assert_eq!(a.candidates, b.candidates);
}
