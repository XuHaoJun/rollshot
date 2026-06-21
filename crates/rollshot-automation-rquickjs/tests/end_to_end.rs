use std::time::Duration;

use rollshot_automation::{
    execute_to_proposal, validate_source, AutomationInput, CancellationFlag, ExecutionError,
    ExecutionMetrics, ExecutionPolicy, FakeAutomationHost, LayoutRegion, OcrMatch, ProposalContext,
    ProposedEditKind, Region, RegionFeatures, TemplateMatch, ValidationLimits,
};
use rollshot_automation_rquickjs::QuickJsExecutor;
use rollshot_edit_proposal::{
    EditProposal, ProposalId, ProposedEdit, Provenance, ProvenanceSource,
};
use rollshot_image_document::{ImagePoint, ImageRect};

fn run_source(
    source: &str,
    host: &mut FakeAutomationHost,
    allowed_kind: ProposedEditKind,
) -> Result<(EditProposal, ExecutionMetrics), ExecutionError> {
    let automation = validate_source(source, &ValidationLimits::default()).unwrap();
    let input = AutomationInput {
        image_width: 100,
        image_height: 100,
        region: Some(Region::Full),
        annotations: Vec::new(),
        capability_handles: Default::default(),
    };
    let proposal = ProposalContext {
        proposal_id: ProposalId(1),
        base_document_state_id: 9,
        provenance: Provenance {
            source: ProvenanceSource::Agent { run_id: 3 },
        },
    };
    let mut policy = ExecutionPolicy::smart_redaction_default(
        Duration::from_secs(1),
        8 * 1024 * 1024,
        256 * 1024,
    );
    policy.allowed_edit_kinds.insert(allowed_kind);
    execute_to_proposal(
        &QuickJsExecutor,
        &automation,
        &input,
        &proposal,
        host,
        &policy,
        &CancellationFlag::new(),
    )
}

#[test]
fn ocr_capability_produces_redaction_proposal() {
    let source = r#"
function expandBounds(rect, padding) {
  return {
    x: rect.x - padding,
    y: rect.y - padding,
    width: rect.width + padding * 2,
    height: rect.height + padding * 2,
  };
}
function main(input) {
  const matches = rollshot.ocr({ region: input.region, limit: 5 });
  return {
    candidates: matches
      .filter((match) => match.confidence > 0.8)
      .map((match) => ({
        kind: "addRedaction",
        bounds: expandBounds(match.bounds, 1),
        confidence: match.confidence,
        label: "ocr-match",
      })),
  };
}
"#;
    let automation = validate_source(source, &ValidationLimits::default()).unwrap();
    let input = AutomationInput {
        image_width: 100,
        image_height: 100,
        region: Some(Region::Full),
        annotations: Vec::new(),
        capability_handles: Default::default(),
    };
    let mut host = FakeAutomationHost {
        ocr_results: vec![OcrMatch {
            bounds: ImageRect::from_corners(
                ImagePoint::new(10.0, 10.0),
                ImagePoint::new(20.0, 20.0),
            ),
            text: "secret@example.com".into(),
            confidence: 0.95,
        }],
        ..Default::default()
    };
    let proposal = ProposalContext {
        proposal_id: ProposalId(1),
        base_document_state_id: 9,
        provenance: Provenance {
            source: ProvenanceSource::Agent { run_id: 3 },
        },
    };
    let policy = ExecutionPolicy::smart_redaction_default(
        Duration::from_secs(1),
        8 * 1024 * 1024,
        256 * 1024,
    );
    let (result, metrics) = execute_to_proposal(
        &QuickJsExecutor,
        &automation,
        &input,
        &proposal,
        &mut host,
        &policy,
        &CancellationFlag::new(),
    )
    .unwrap();
    assert_eq!(result.candidates.len(), 1);
    assert_eq!(result.candidates[0].label, "ocr-match");
    assert_eq!(metrics.capability_calls, 1);
}

#[test]
fn layout_capability_produces_text_note() {
    let source = r#"
function main(input) {
  const regions = rollshot.layout({ region: input.region, limit: 1 });
  return {
    candidates: regions.map((region) => ({
      kind: "addTextNote",
      position: { x: region.bounds.x, y: region.bounds.y },
      text: region.role,
      confidence: region.confidence,
      label: "layout-region",
    })),
  };
}
"#;
    let bounds = ImageRect::from_corners(ImagePoint::new(5.0, 6.0), ImagePoint::new(20.0, 16.0));
    let mut host = FakeAutomationHost {
        layout_results: vec![LayoutRegion {
            bounds,
            role: "dialog".into(),
            confidence: 0.9,
        }],
        ..Default::default()
    };
    let (proposal, metrics) = run_source(source, &mut host, ProposedEditKind::AddTextNote).unwrap();
    assert!(matches!(
        &proposal.candidates[0].edit,
        ProposedEdit::AddTextNote { position, text }
            if *position == ImagePoint::new(5.0, 6.0) && text == "dialog"
    ));
    assert_eq!(metrics.capability_calls, 1);
}

#[test]
fn region_features_capability_uses_pure_geometry_helper() {
    let source = r#"
function expand(rect) {
  return {
    x: rect.x - 1,
    y: rect.y - 1,
    width: rect.width + 2,
    height: rect.height + 2,
  };
}
function main(input) {
  const features = rollshot.regionFeatures({ region: input.region, limit: 1 });
  return {
    candidates: features.map((feature) => ({
      kind: "addRedaction",
      bounds: expand(feature.bounds),
      confidence: 0.9,
      label: "feature-region",
    })),
  };
}
"#;
    let mut host = FakeAutomationHost {
        region_feature_results: vec![RegionFeatures {
            bounds: ImageRect::from_corners(
                ImagePoint::new(10.0, 10.0),
                ImagePoint::new(20.0, 20.0),
            ),
            dominant_rgba: [0, 0, 0, 255],
            edge_density: 0.5,
        }],
        ..Default::default()
    };
    let (proposal, metrics) =
        run_source(source, &mut host, ProposedEditKind::AddRedaction).unwrap();
    assert!(matches!(
        proposal.candidates[0].edit,
        ProposedEdit::AddRedaction { bounds }
            if bounds
                == ImageRect::from_corners(
                    ImagePoint::new(9.0, 9.0),
                    ImagePoint::new(21.0, 21.0),
                )
    ));
    assert_eq!(metrics.capability_calls, 1);
}

#[test]
fn template_match_capability_produces_number_callout() {
    let source = r#"
function main(input) {
  const matches = rollshot.templateMatch({
    templateHandle: "profile",
    region: input.region,
    limit: 1,
  });
  return {
    candidates: matches.map((match) => ({
      kind: "addNumberCallout",
      tip: match.anchor,
      bubble: { x: match.bounds.x, y: match.bounds.y },
      confidence: match.score,
      label: "template-match",
    })),
  };
}
"#;
    let mut host = FakeAutomationHost {
        template_results: vec![TemplateMatch {
            bounds: ImageRect::from_corners(
                ImagePoint::new(30.0, 40.0),
                ImagePoint::new(50.0, 60.0),
            ),
            score: 0.95,
            anchor: ImagePoint::new(35.0, 45.0),
        }],
        ..Default::default()
    };
    let (proposal, metrics) =
        run_source(source, &mut host, ProposedEditKind::AddNumberCallout).unwrap();
    assert!(matches!(
        proposal.candidates[0].edit,
        ProposedEdit::AddNumberCallout { tip, bubble }
            if tip == ImagePoint::new(35.0, 45.0)
                && bubble == ImagePoint::new(30.0, 40.0)
    ));
    assert_eq!(metrics.capability_calls, 1);
}

#[test]
fn capability_error_remains_typed() {
    use rollshot_automation::CapabilityError;

    let source = r#"
function main(input) {
  const matches = rollshot.ocr({ region: input.region, limit: 1 });
  return { candidates: matches };
}
"#;
    let mut host = FakeAutomationHost {
        failure: Some(CapabilityError::Failed {
            code: "fixture_failure",
        }),
        ..Default::default()
    };
    assert_eq!(
        run_source(source, &mut host, ProposedEditKind::AddRedaction),
        Err(ExecutionError::Capability(CapabilityError::Failed {
            code: "fixture_failure",
        }))
    );
}
