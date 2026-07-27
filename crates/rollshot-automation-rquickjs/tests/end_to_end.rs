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

struct UnboundedHost {
    results: Vec<OcrMatch>,
}

impl rollshot_automation::AutomationHost for UnboundedHost {
    fn ocr(
        &mut self,
        _query: rollshot_automation::OcrQuery,
    ) -> Result<Vec<OcrMatch>, rollshot_automation::CapabilityError> {
        Ok(self.results.clone())
    }

    fn layout(
        &mut self,
        _query: rollshot_automation::LayoutQuery,
    ) -> Result<Vec<LayoutRegion>, rollshot_automation::CapabilityError> {
        Ok(Vec::new())
    }

    fn region_features(
        &mut self,
        _query: rollshot_automation::RegionFeaturesQuery,
    ) -> Result<Vec<RegionFeatures>, rollshot_automation::CapabilityError> {
        Ok(Vec::new())
    }

    fn template_match(
        &mut self,
        _query: rollshot_automation::TemplateMatchQuery,
    ) -> Result<Vec<TemplateMatch>, rollshot_automation::CapabilityError> {
        Ok(Vec::new())
    }
}

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
        proposal_id: ProposalId::parse("proposal-00000001-0000-4000-8000-000000000000").unwrap(),
        base_document_state_id: 9,
        provenance: Provenance {
            source: ProvenanceSource::Agent { run_id: "run-00000000-0000-4000-8000-000000000003".to_string() },
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
            quad: [
                ImagePoint { x: 10.0, y: 10.0 },
                ImagePoint { x: 20.0, y: 10.0 },
                ImagePoint { x: 20.0, y: 20.0 },
                ImagePoint { x: 10.0, y: 20.0 },
            ],
            text: "secret@example.com".into(),
            confidence: 0.95,
        }],
        ..Default::default()
    };
    let proposal = ProposalContext {
        proposal_id: ProposalId::parse("proposal-00000001-0000-4000-8000-000000000000").unwrap(),
        base_document_state_id: 9,
        provenance: Provenance {
            source: ProvenanceSource::Agent { run_id: "run-00000000-0000-4000-8000-000000000003".to_string() },
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

#[test]
fn bridge_truncates_host_results_to_query_limit() {
    let source = r#"
function main(input) {
  const matches = rollshot.ocr({ region: input.region, limit: 1 });
  return {
    candidates: matches.map((match) => ({
      kind: "addRedaction",
      bounds: match.bounds,
      confidence: match.confidence,
      label: "match",
    })),
  };
}
"#;
    let bounds = ImageRect::from_corners(ImagePoint::new(1.0, 1.0), ImagePoint::new(2.0, 2.0));
    let mut host = UnboundedHost {
        results: vec![
            OcrMatch {
                bounds,
                quad: [
                    ImagePoint {
                        x: bounds.x,
                        y: bounds.y,
                    },
                    ImagePoint {
                        x: bounds.x + bounds.width,
                        y: bounds.y,
                    },
                    ImagePoint {
                        x: bounds.x + bounds.width,
                        y: bounds.y + bounds.height,
                    },
                    ImagePoint {
                        x: bounds.x,
                        y: bounds.y + bounds.height,
                    },
                ],
                text: "one".into(),
                confidence: 1.0,
            },
            OcrMatch {
                bounds,
                quad: [
                    ImagePoint {
                        x: bounds.x,
                        y: bounds.y,
                    },
                    ImagePoint {
                        x: bounds.x + bounds.width,
                        y: bounds.y,
                    },
                    ImagePoint {
                        x: bounds.x + bounds.width,
                        y: bounds.y + bounds.height,
                    },
                    ImagePoint {
                        x: bounds.x,
                        y: bounds.y + bounds.height,
                    },
                ],
                text: "two".into(),
                confidence: 1.0,
            },
        ],
    };
    let automation = validate_source(source, &ValidationLimits::default()).unwrap();
    let input = AutomationInput {
        image_width: 100,
        image_height: 100,
        region: Some(Region::Full),
        annotations: Vec::new(),
        capability_handles: Default::default(),
    };
    let proposal = ProposalContext {
        proposal_id: ProposalId::parse("proposal-00000001-0000-4000-8000-000000000000").unwrap(),
        base_document_state_id: 0,
        provenance: Provenance {
            source: ProvenanceSource::Agent { run_id: "run-00000000-0000-4000-8000-000000000001".to_string() },
        },
    };
    let policy = ExecutionPolicy::smart_redaction_default(
        Duration::from_secs(1),
        8 * 1024 * 1024,
        256 * 1024,
    );
    let (proposal, _) = execute_to_proposal(
        &QuickJsExecutor,
        &automation,
        &input,
        &proposal,
        &mut host,
        &policy,
        &CancellationFlag::new(),
    )
    .unwrap();
    assert_eq!(proposal.candidates.len(), 1);
}

#[test]
fn bridge_rejects_non_finite_host_geometry() {
    let source = r#"
function main(input) {
  rollshot.ocr({ region: input.region, limit: 1 });
  return { candidates: [] };
}
"#;
    let mut host = FakeAutomationHost {
        ocr_results: vec![OcrMatch {
            bounds: ImageRect {
                x: f32::NAN,
                y: 0.0,
                width: 1.0,
                height: 1.0,
            },
            quad: [
                ImagePoint {
                    x: f32::NAN,
                    y: 0.0,
                },
                ImagePoint {
                    x: f32::NAN,
                    y: 0.0,
                },
                ImagePoint {
                    x: f32::NAN,
                    y: 1.0,
                },
                ImagePoint {
                    x: f32::NAN,
                    y: 1.0,
                },
            ],
            text: "bad".into(),
            confidence: 1.0,
        }],
        ..Default::default()
    };
    assert_eq!(
        run_source(source, &mut host, ProposedEditKind::AddRedaction),
        Err(ExecutionError::Capability(
            rollshot_automation::CapabilityError::Failed {
                code: "invalid_value"
            }
        ))
    );
}
