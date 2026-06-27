use std::sync::{Arc, Mutex as StdMutex};

use rollshot_agent::runtime::RunBudget;
use rollshot_automation::{
    execute_to_proposal, AutomationInput, CancellationFlag, ExecutionPolicy, ProposalContext,
};
use rollshot_automation_rquickjs::QuickJsExecutor;
use rollshot_edit_proposal::{EditProposal, ProposalId, Provenance, ProvenanceSource};
use rollshot_preset::AutomationRevision;
use rollshot_vision::VisualIndex;

use super::state::WorkbenchError;
use super::PayloadMode;

const PHASE_A_REGION_FEATURE_STRIP_PX: u32 = 96;
const PHASE_A_REGION_FEATURE_LIMIT: u32 = 1;
const PHASE_A_REGION_FEATURE_FULL_AREA_LIMIT: u64 = 8_000_000;

const PHASE_B2_OCR_STRIP_PX: u32 = 96;
const PHASE_B2_OCR_LIMIT: u32 = 50;
const PHASE_B2_OCR_AREA_LIMIT: u64 = 16_000_000;

#[derive(Debug, Clone, PartialEq)]
struct CanonicalOcrEntry {
    name: &'static str,
    bounds: rollshot_image_document::ImageRect,
    query: Option<rollshot_automation::OcrQuery>,
    unavailable_reason: Option<&'static str>,
}

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

#[derive(Debug, Clone, PartialEq)]
struct CanonicalRegionFeatureEntry {
    name: &'static str,
    bounds: rollshot_image_document::ImageRect,
    query: Option<rollshot_automation::RegionFeaturesQuery>,
    unavailable_reason: Option<&'static str>,
}

fn canonical_region_feature_catalog(width: u32, height: u32) -> Vec<CanonicalRegionFeatureEntry> {
    use rollshot_automation::{Region, RegionFeaturesQuery};
    use rollshot_image_document::ImageRect;

    if width == 0 || height == 0 {
        return Vec::new();
    }

    let width_f = width as f32;
    let height_f = height as f32;
    let strip_h = height.min(PHASE_A_REGION_FEATURE_STRIP_PX) as f32;
    let strip_w = width.min(PHASE_A_REGION_FEATURE_STRIP_PX) as f32;

    let make_entry = |name: &'static str, bounds: ImageRect| {
        let area = (bounds.width.ceil() as u64).saturating_mul(bounds.height.ceil() as u64);
        if area > PHASE_A_REGION_FEATURE_FULL_AREA_LIMIT {
            CanonicalRegionFeatureEntry {
                name,
                bounds,
                query: None,
                unavailable_reason: Some("area_limit_exceeded"),
            }
        } else {
            CanonicalRegionFeatureEntry {
                name,
                bounds,
                query: Some(RegionFeaturesQuery {
                    region: Region::Rect { bounds },
                    limit: PHASE_A_REGION_FEATURE_LIMIT,
                }),
                unavailable_reason: None,
            }
        }
    };

    vec![
        CanonicalRegionFeatureEntry {
            name: "full",
            bounds: ImageRect {
                x: 0.0,
                y: 0.0,
                width: width_f,
                height: height_f,
            },
            query: if (width as u64 * height as u64) <= PHASE_A_REGION_FEATURE_FULL_AREA_LIMIT {
                Some(RegionFeaturesQuery {
                    region: Region::Full,
                    limit: PHASE_A_REGION_FEATURE_LIMIT,
                })
            } else {
                None
            },
            unavailable_reason: if (width as u64 * height as u64)
                <= PHASE_A_REGION_FEATURE_FULL_AREA_LIMIT
            {
                None
            } else {
                Some("area_limit_exceeded")
            },
        },
        make_entry(
            "top_strip",
            ImageRect {
                x: 0.0,
                y: 0.0,
                width: width_f,
                height: strip_h,
            },
        ),
        make_entry(
            "left_strip",
            ImageRect {
                x: 0.0,
                y: 0.0,
                width: strip_w,
                height: height_f,
            },
        ),
        make_entry(
            "right_strip",
            ImageRect {
                x: (width_f - strip_w).max(0.0),
                y: 0.0,
                width: strip_w,
                height: height_f,
            },
        ),
        make_entry(
            "bottom_strip",
            ImageRect {
                x: 0.0,
                y: (height_f - strip_h).max(0.0),
                width: width_f,
                height: strip_h,
            },
        ),
    ]
}

fn canonical_ocr_catalog(width: u32, height: u32) -> Vec<CanonicalOcrEntry> {
    use rollshot_automation::{OcrQuery, Region};
    use rollshot_image_document::ImageRect;

    if width == 0 || height == 0 {
        return Vec::new();
    }

    let width_f = width as f32;
    let height_f = height as f32;
    let strip_h = height.min(PHASE_B2_OCR_STRIP_PX) as f32;
    let strip_w = width.min(PHASE_B2_OCR_STRIP_PX) as f32;

    let make_entry = |name: &'static str, bounds: ImageRect| {
        let area = (bounds.width.ceil() as u64).saturating_mul(bounds.height.ceil() as u64);
        if area > PHASE_B2_OCR_AREA_LIMIT {
            CanonicalOcrEntry {
                name,
                bounds,
                query: None,
                unavailable_reason: Some("area_limit_exceeded"),
            }
        } else {
            CanonicalOcrEntry {
                name,
                bounds,
                query: Some(OcrQuery {
                    region: Region::Rect { bounds },
                    limit: PHASE_B2_OCR_LIMIT,
                }),
                unavailable_reason: None,
            }
        }
    };

    vec![
        CanonicalOcrEntry {
            name: "full",
            bounds: ImageRect {
                x: 0.0,
                y: 0.0,
                width: width_f,
                height: height_f,
            },
            query: if (width as u64 * height as u64) <= PHASE_B2_OCR_AREA_LIMIT {
                Some(OcrQuery {
                    region: Region::Full,
                    limit: PHASE_B2_OCR_LIMIT,
                })
            } else {
                None
            },
            unavailable_reason: if (width as u64 * height as u64) <= PHASE_B2_OCR_AREA_LIMIT {
                None
            } else {
                Some("area_limit_exceeded")
            },
        },
        make_entry(
            "top_strip",
            ImageRect {
                x: 0.0,
                y: 0.0,
                width: width_f,
                height: strip_h,
            },
        ),
        make_entry(
            "left_strip",
            ImageRect {
                x: 0.0,
                y: 0.0,
                width: strip_w,
                height: height_f,
            },
        ),
        make_entry(
            "right_strip",
            ImageRect {
                x: (width_f - strip_w).max(0.0),
                y: 0.0,
                width: strip_w,
                height: height_f,
            },
        ),
        make_entry(
            "bottom_strip",
            ImageRect {
                x: 0.0,
                y: (height_f - strip_h).max(0.0),
                width: width_f,
                height: strip_h,
            },
        ),
    ]
}

fn product_capability_handles() -> std::collections::BTreeMap<String, String> {
    std::collections::BTreeMap::new()
}

fn authoring_inspection_context(
    payload_mode: PayloadMode,
    catalog: &[CanonicalRegionFeatureEntry],
    ocr_catalog: &[CanonicalOcrEntry],
) -> rollshot_agent::tools::AuthoringInspectionContext {
    let regions = catalog
        .iter()
        .map(|entry| rollshot_agent::tools::CanonicalRegionInspection {
            name: entry.name.into(),
            bounds: Some(entry.bounds),
            query: entry.query.clone(),
            unavailable_reason: entry.unavailable_reason.map(str::to_string),
        })
        .collect();

    #[cfg(feature = "ocr")]
    let ocr_regions = ocr_catalog
        .iter()
        .map(|entry| rollshot_agent::tools::CanonicalOcrInspection {
            name: entry.name.into(),
            bounds: Some(entry.bounds),
            query: entry.query.clone(),
            unavailable_reason: entry.unavailable_reason.map(str::to_string),
        })
        .collect();
    #[cfg(not(feature = "ocr"))]
    let ocr_regions = {
        let _ = ocr_catalog;
        Vec::new()
    };

    let payload_mode = match payload_mode {
        PayloadMode::FullScreenshot => "full_screenshot",
        PayloadMode::OcrLayoutOnly => "ocr_layout_only",
    };

    rollshot_agent::tools::AuthoringInspectionContext {
        payload_mode: payload_mode.into(),
        regions,
        ocr_regions,
        ocr_status: if cfg!(feature = "ocr") {
            rollshot_agent::tools::CapabilityStatus::unavailable("no_prepared_ocr_regions")
        } else {
            rollshot_agent::tools::CapabilityStatus::unavailable("ocr_disabled")
        },
        layout_status: rollshot_agent::tools::CapabilityStatus::unavailable(
            "capability_unavailable",
        ),
        template_match_status: rollshot_agent::tools::CapabilityStatus::unavailable(
            "no_capability_handles",
        ),
    }
}

fn prepare_phase_a_region_features(
    host: &mut rollshot_vision::RealAutomationHost,
    index: &VisualIndex,
) -> Result<(), WorkbenchError> {
    for entry in canonical_region_feature_catalog(index.width(), index.height()) {
        let Some(query) = entry.query else {
            continue;
        };
        host.prepare_region_features(index, &query)
            .map_err(|e| WorkbenchError::VisionPrepare {
                message: format!("regionFeatures {}: {e}", entry.name),
            })?;
    }
    Ok(())
}

#[cfg(feature = "ocr")]
fn prepare_phase_b2_ocr(
    host: &mut rollshot_vision::RealAutomationHost,
    index: &VisualIndex,
) -> Result<(), WorkbenchError> {
    for entry in canonical_ocr_catalog(index.width(), index.height()) {
        let Some(query) = entry.query else {
            continue;
        };
        host.prepare_ocr(index, &query)
            .map_err(|e| WorkbenchError::VisionPrepare {
                message: format!("ocr {}: {e}", entry.name),
            })?;
    }
    Ok(())
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
    let index = VisualIndex::build(image.clone()).map_err(|e| WorkbenchError::VisionPrepare {
        message: format!("VisualIndex: {e}"),
    })?;
    let mut host = rollshot_vision::RealAutomationHost::new();
    prepare_phase_a_region_features(&mut host, &index)?;
    #[cfg(feature = "ocr")]
    prepare_phase_b2_ocr(&mut host, &index)?;
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

pub fn prepare_vision_context(
    image: &image::RgbaImage,
) -> Result<super::VisionContext, WorkbenchError> {
    let index = VisualIndex::build(image.clone()).map_err(|e| WorkbenchError::VisionPrepare {
        message: format!("VisualIndex: {e}"),
    })?;
    let mut host = rollshot_vision::RealAutomationHost::new();
    prepare_phase_a_region_features(&mut host, &index)?;
    #[cfg(feature = "ocr")]
    prepare_phase_b2_ocr(&mut host, &index)?;
    Ok(super::VisionContext {
        index,
        host: Arc::new(StdMutex::new(host)),
        executor: QuickJsExecutor,
        cancellation: rollshot_automation::CancellationFlag::default(),
    })
}

struct ChannelEventSink {
    tx: tokio::sync::mpsc::Sender<rollshot_agent::runtime::RunEvent>,
}

impl rollshot_agent::runtime::RunEventSink for ChannelEventSink {
    fn emit(&self, event: rollshot_agent::runtime::RunEvent) {
        let _ = self.tx.try_send(event);
    }
}

fn build_authoring_tool_registry(
    tool_ctx: Arc<rollshot_agent::tools::ToolContext>,
    executor: Arc<dyn rollshot_automation::AutomationExecutor>,
    host: Arc<StdMutex<dyn rollshot_automation::AutomationHost>>,
    inspection: rollshot_agent::tools::AuthoringInspectionContext,
) -> Result<rollshot_agent::tools::ToolRegistry, WorkbenchError> {
    #[cfg(feature = "ocr")]
    use rollshot_agent::tools::OcrTool;
    use rollshot_agent::tools::{
        DryRunTool, GetContextSummaryTool, InspectImageContextTool, RegionFeaturesTool,
        ReplaceSourceTool, RequestUserInputTool, SubmitForReviewTool, ToolRegistry,
        ToolRegistryLimits, ValidateSourceTool,
    };

    let mut registry = ToolRegistry::new(ToolRegistryLimits::permissive());
    let reg = |registry: &mut ToolRegistry,
               tool: Arc<dyn rollshot_agent::tools::Tool>|
     -> Result<(), WorkbenchError> {
        registry
            .register(tool)
            .map_err(|_| WorkbenchError::RuntimeFailure)
    };

    reg(
        &mut registry,
        Arc::new(ReplaceSourceTool::new(tool_ctx.clone())),
    )?;
    reg(
        &mut registry,
        Arc::new(ValidateSourceTool::new(tool_ctx.clone())),
    )?;
    reg(
        &mut registry,
        Arc::new(SubmitForReviewTool::new(tool_ctx.clone())),
    )?;
    reg(
        &mut registry,
        Arc::new(RequestUserInputTool::new(tool_ctx.clone())),
    )?;
    reg(
        &mut registry,
        Arc::new(GetContextSummaryTool::new(tool_ctx.clone())),
    )?;
    reg(
        &mut registry,
        Arc::new(InspectImageContextTool::new(
            tool_ctx.clone(),
            inspection.clone(),
        )),
    )?;
    reg(
        &mut registry,
        Arc::new(RegionFeaturesTool::new(
            tool_ctx.clone(),
            host.clone(),
            inspection.regions.clone(),
        )),
    )?;
    #[cfg(feature = "ocr")]
    reg(
        &mut registry,
        Arc::new(OcrTool::new(
            tool_ctx.clone(),
            host.clone(),
            inspection.ocr_regions.clone(),
        )),
    )?;
    reg(
        &mut registry,
        Arc::new(DryRunTool::new(tool_ctx, executor, host)),
    )?;

    Ok(registry)
}

/// Start a bounded agent run as an iced `Task` that streams `RunEvent`s and
/// emits a final `RunTerminal`. The `AgentSession` is moved into the spawned
/// task by value (not held in any Mutex) so the spawned future stays `Send`
/// across `.await`.
///
/// Vision-prep + PNG-encode happen inside the spawned async task (not on the
/// UI thread). The `payload_mode` gates whether image bytes are uploaded.
pub fn start_agent_run(
    params: &super::PendingRunParams,
    image: &image::RgbaImage,
    provider_config: &super::provider_config::ProviderConfig,
    budget: &RunBudget,
    session: rollshot_agent::domain::AgentSession,
    payload_mode: PayloadMode,
) -> Result<
    (
        iced::Task<crate::result_workspace::Message>,
        rollshot_agent::runtime::RunCancellation,
    ),
    WorkbenchError,
> {
    use rollshot_agent::{
        domain::{AttachmentDescriptor, AuthorizedModelInput, MediaType},
        driver::{AgentConfig, AgentRunner},
        runtime::{RunCancellation, RunEvent},
    };

    if !super::provider_config::has_key(provider_config) {
        return Err(WorkbenchError::Config);
    }

    let adapter = super::provider_config::build_adapter(provider_config)
        .map_err(|_| WorkbenchError::Config)?;

    let provider_string = provider_config.provider.to_string().to_lowercase();
    let model_string = provider_config.model.clone();
    let session_id = session.session_id;
    let user_message = params.user_message.clone();
    let image_dims = params.image_dims;
    let active_source = params.active_revision_source.clone().unwrap_or_default();
    let image = image.clone();
    let budget = budget.clone();

    let cancellation = RunCancellation::new();
    let cancellation_for_task = cancellation.clone();

    let stream = async_stream::stream! {
        // Heavy work runs inside the spawned task (B5).
        let vision = match prepare_vision_context(&image) {
            Ok(v) => v,
            Err(e) => {
                yield crate::result_workspace::Message::Workbench(
                    super::WorkbenchMessage::RunFailed(e),
                );
                return;
            }
        };

        let validation_limits = rollshot_automation::ValidationLimits::default();
        let policy = rollshot_automation::ExecutionPolicy::smart_redaction_default(
            std::time::Duration::from_secs(25), 80_000_000, 8_000_000,
        );
        let tool_ctx = Arc::new(rollshot_agent::tools::ToolContext::new_with_capability_handles(
            session_id,
            active_source,
            validation_limits,
            policy,
            image_dims,
            product_capability_handles(),
            &cancellation_for_task,
        ));

        let region_catalog = canonical_region_feature_catalog(image_dims.0, image_dims.1);
        let ocr_catalog = canonical_ocr_catalog(image_dims.0, image_dims.1);
        let inspection = authoring_inspection_context(
            payload_mode,
            &region_catalog,
            &ocr_catalog,
        );
        let registry = match build_authoring_tool_registry(
            tool_ctx.clone(),
            Arc::new(vision.executor),
            vision.host.clone() as Arc<StdMutex<dyn rollshot_automation::AutomationHost>>,
            inspection,
        ) {
            Ok(registry) => registry,
            Err(e) => {
                yield crate::result_workspace::Message::Workbench(
                    super::WorkbenchMessage::RunFailed(e),
                );
                return;
            }
        };

        // C6: payload_mode gates the bytes.
        let (descriptors, attachment_bytes) = match payload_mode {
            PayloadMode::OcrLayoutOnly => (vec![], vec![]),
            PayloadMode::FullScreenshot => {
                let mut buf = Vec::new();
                if let Err(e) = image::DynamicImage::ImageRgba8(image.clone())
                    .write_to(&mut std::io::Cursor::new(&mut buf), image::ImageFormat::Png)
                {
                    yield crate::result_workspace::Message::Workbench(
                        super::WorkbenchMessage::RunFailed(WorkbenchError::VisionPrepare {
                            message: format!("png encode: {e}"),
                        }),
                    );
                    return;
                }
                let descriptor = AttachmentDescriptor {
                    media_type: MediaType::Png,
                    width: image_dims.0,
                    height: image_dims.1,
                    byte_count: buf.len() as u64,
                };
                (vec![descriptor], vec![buf])
            }
        };

        let model_input = match AuthorizedModelInput::new(
            provider_string,
            model_string,
            user_message,
            descriptors,
            attachment_bytes,
        ) {
            Ok(input) => input,
            Err(_) => {
                yield crate::result_workspace::Message::Workbench(
                    super::WorkbenchMessage::RunFailed(WorkbenchError::RuntimeFailure),
                );
                return;
            }
        };

        let runner = AgentRunner::new(AgentConfig::default());
        let (tx, mut rx) = tokio::sync::mpsc::channel::<RunEvent>(64);
        let sink = ChannelEventSink { tx };

        // B4: tokio::spawn inside the stream block (runtime context).
        let run_task = tokio::spawn(async move {
            let mut session = session;
            runner.run_with_provider(
                model_input, &mut session, &registry, budget,
                &cancellation_for_task, &sink, &tool_ctx, adapter.as_ref(),
            ).await
        });

        while let Some(event) = rx.recv().await {
            yield crate::result_workspace::Message::Workbench(
                super::WorkbenchMessage::RunEvent(event),
            );
        }
        if let Ok(terminal) = run_task.await {
            yield crate::result_workspace::Message::Workbench(
                super::WorkbenchMessage::RunTerminal(terminal),
            );
        }
    };

    let task = iced::Task::run(stream, std::convert::identity);
    Ok((task, cancellation))
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

    fn make_revision_from_source(source: &str) -> AutomationRevision {
        use rollshot_preset::*;
        let limits = rollshot_automation::ValidationLimits::default();
        let validated = rollshot_automation::validate_source(source, &limits).unwrap();
        AutomationRevision {
            store_schema_version: STORE_SCHEMA_VERSION,
            id: RevisionId("rev-1".into()),
            preset_id: PresetId("test".into()),
            parent_id: None,
            created_at: "2026-06-27T00:00:00Z".into(),
            provenance: RevisionProvenance {
                origin: RevisionOrigin::Manual,
                note: None,
                source_run_ref: None,
            },
            artifact: validated,
        }
    }

    #[test]
    fn run_existing_preset_prepares_top_strip_region_features() {
        let source = r#"
function main(input) {
  const bounds = { x: 0, y: 0, width: input.imageWidth, height: Math.min(96, input.imageHeight) };
  const features = rollshot.regionFeatures({ region: { kind: "rect", bounds: bounds }, limit: 1 });
  const hasFeatures = features.length > 0;
  return {
    candidates: hasFeatures ? [{
      kind: "addRedaction",
      bounds: bounds,
      confidence: 0.6,
      label: "top-strip"
    }] : []
  };
}
"#;
        let image = image::RgbaImage::from_pixel(160, 120, image::Rgba([30, 30, 30, 255]));
        let revision = make_revision_from_source(source);
        let policy = ExecutionPolicy::smart_redaction_default(
            std::time::Duration::from_secs(10),
            100_000_000,
            8_000_000,
        );

        let proposal = run_existing_preset(&image, &revision, &policy).unwrap();

        assert_eq!(proposal.candidates.len(), 1);
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

#[cfg(test)]
mod prepare_tests {
    use super::*;

    #[test]
    fn prepare_vision_context_rejects_empty_image() {
        let empty = image::RgbaImage::new(0, 0);
        let r = prepare_vision_context(&empty);
        assert!(matches!(r, Err(WorkbenchError::VisionPrepare { .. })));
    }

    #[test]
    fn prepare_vision_context_succeeds_for_valid_image() {
        let img = image::RgbaImage::from_fn(8, 8, |_, _| image::Rgba([200, 200, 200, 255]));
        let ctx = prepare_vision_context(&img).unwrap();
        assert_eq!(ctx.index.width(), 8);
        assert_eq!(ctx.index.height(), 8);
    }

    #[test]
    fn canonical_region_catalog_matches_prompt_top_strip() {
        let catalog = canonical_region_feature_catalog(160, 120);
        let top = catalog
            .iter()
            .find(|entry| entry.name == "top_strip")
            .expect("top strip entry");
        assert_eq!(
            top.bounds,
            rollshot_image_document::ImageRect {
                x: 0.0,
                y: 0.0,
                width: 160.0,
                height: 96.0,
            }
        );
        assert!(top.query.is_some());
        assert_eq!(top.unavailable_reason, None);
    }

    #[test]
    fn canonical_region_catalog_keeps_skipped_full_region_with_reason() {
        let catalog = canonical_region_feature_catalog(10_000, 10_000);
        let full = catalog
            .iter()
            .find(|entry| entry.name == "full")
            .expect("full entry");
        assert_eq!(full.query, None);
        assert_eq!(full.unavailable_reason, Some("area_limit_exceeded"));
    }

    #[test]
    fn canonical_region_catalog_has_named_entries_for_every_region() {
        let names: Vec<&str> = canonical_region_feature_catalog(160, 120)
            .iter()
            .map(|entry| entry.name)
            .collect();
        assert_eq!(
            names,
            vec![
                "full",
                "top_strip",
                "left_strip",
                "right_strip",
                "bottom_strip"
            ]
        );
    }

    #[test]
    fn canonical_ocr_catalog_has_named_entries_for_every_region() {
        let names: Vec<&str> = canonical_ocr_catalog(160, 120)
            .iter()
            .map(|entry| entry.name)
            .collect();
        assert_eq!(
            names,
            vec![
                "full",
                "top_strip",
                "left_strip",
                "right_strip",
                "bottom_strip"
            ]
        );
    }

    #[test]
    fn canonical_ocr_catalog_prefers_full_region_when_under_cap() {
        let catalog = canonical_ocr_catalog(160, 120);
        let full = catalog
            .iter()
            .find(|entry| entry.name == "full")
            .expect("full OCR entry");
        assert_eq!(
            full.bounds,
            rollshot_image_document::ImageRect {
                x: 0.0,
                y: 0.0,
                width: 160.0,
                height: 120.0,
            }
        );
        assert!(full.query.is_some());
        assert_eq!(full.unavailable_reason, None);
    }

    #[test]
    fn canonical_ocr_catalog_keeps_oversized_regions_with_reason() {
        let catalog = canonical_ocr_catalog(100_000, 100_000);
        let full = catalog
            .iter()
            .find(|entry| entry.name == "full")
            .expect("full OCR entry");
        assert_eq!(full.query, None);
        assert_eq!(full.unavailable_reason, Some("area_limit_exceeded"));
    }

    #[test]
    fn canonical_region_catalog_never_prepares_region_over_area_cap() {
        for entry in canonical_region_feature_catalog(100_000, 100_000) {
            if let Some(query) = &entry.query {
                let area = match query.region {
                    rollshot_automation::Region::Full => 100_000_u64 * 100_000_u64,
                    rollshot_automation::Region::Rect { bounds } => {
                        (bounds.width.ceil() as u64).saturating_mul(bounds.height.ceil() as u64)
                    }
                };
                assert!(
                    area <= PHASE_A_REGION_FEATURE_FULL_AREA_LIMIT,
                    "{} prepared area {} exceeds cap {}",
                    entry.name,
                    area,
                    PHASE_A_REGION_FEATURE_FULL_AREA_LIMIT
                );
            } else {
                assert_eq!(entry.unavailable_reason, Some("area_limit_exceeded"));
            }
        }
    }

    fn tool_context_for_tests() -> std::sync::Arc<rollshot_agent::tools::ToolContext> {
        let cancel = rollshot_agent::runtime::RunCancellation::new();
        std::sync::Arc::new(rollshot_agent::tools::ToolContext::new_with_capability_handles(
            rollshot_agent::domain::SessionId::new(1),
            String::new(),
            rollshot_automation::ValidationLimits::default(),
            rollshot_automation::ExecutionPolicy::smart_redaction_default(
                std::time::Duration::from_secs(5),
                4 * 1024 * 1024,
                1024 * 1024,
            ),
            (64, 64),
            product_capability_handles(),
            &cancel,
        ))
    }

    #[test]
    fn authoring_registry_exposes_truthful_phase_b1_tools() {
        assert!(product_capability_handles().is_empty());
        let ctx = tool_context_for_tests();
        let executor: std::sync::Arc<dyn rollshot_automation::AutomationExecutor> =
            std::sync::Arc::new(rollshot_automation_rquickjs::QuickJsExecutor);
        let host: std::sync::Arc<std::sync::Mutex<dyn rollshot_automation::AutomationHost>> =
            std::sync::Arc::new(std::sync::Mutex::new(
                rollshot_automation::FakeAutomationHost::default(),
            ));
        let inspection = authoring_inspection_context(
            PayloadMode::FullScreenshot,
            &canonical_region_feature_catalog(64, 64),
            &canonical_ocr_catalog(64, 64),
        );

        let registry = build_authoring_tool_registry(ctx, executor, host, inspection).unwrap();
        let names = registry.tool_names();

        assert_eq!(
            names,
            vec![
                "replace_source",
                "validate_source",
                "submit_for_review",
                "request_user_input",
                "inspect_context_summary",
                "inspect_image_context",
                "inspect_region_features",
                "dry_run",
            ]
        );
        assert!(!names.contains(&"inspect_ocr"));
        assert!(!names.contains(&"inspect_layout"));
    }

    #[cfg(not(feature = "ocr"))]
    #[tokio::test]
    async fn default_build_inspection_reports_ocr_disabled() {
        use rollshot_agent::tools::{InspectImageContextTool, Tool};

        let ctx = tool_context_for_tests();
        let inspection = authoring_inspection_context(
            PayloadMode::FullScreenshot,
            &canonical_region_feature_catalog(64, 64),
            &canonical_ocr_catalog(64, 64),
        );
        let tool = InspectImageContextTool::new(ctx, inspection);

        let result = tool.call(&serde_json::json!({})).await.unwrap();

        match result {
            rollshot_agent::tools::ToolOutcome::Success { result_json } => {
                assert_eq!(
                    result_json["capabilities"]["ocr"]["status"].as_str(),
                    Some("unavailable")
                );
                assert_eq!(
                    result_json["capabilities"]["ocr"]["reason"].as_str(),
                    Some("ocr_disabled")
                );
                assert!(
                    result_json["ocr_regions"].as_array().unwrap().is_empty(),
                    "default builds must not advertise prepared OCR regions"
                );
                assert!(result_json["capability_handles"].as_array().unwrap().is_empty());
                assert_eq!(
                    result_json["capabilities"]["template_match"]["reason"].as_str(),
                    Some("no_capability_handles")
                );
            }
            other => panic!("expected inspection success, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn product_inspection_reports_template_match_unavailable_without_handles() {
        use rollshot_agent::tools::{InspectImageContextTool, Tool};

        let ctx = tool_context_for_tests();
        let inspection = authoring_inspection_context(
            PayloadMode::FullScreenshot,
            &canonical_region_feature_catalog(64, 64),
            &canonical_ocr_catalog(64, 64),
        );
        let tool = InspectImageContextTool::new(ctx, inspection);

        let result = tool.call(&serde_json::json!({})).await.unwrap();

        match result {
            rollshot_agent::tools::ToolOutcome::Success { result_json } => {
                assert!(result_json["capability_handles"].as_array().unwrap().is_empty());
                assert_eq!(
                    result_json["capabilities"]["template_match"]["status"].as_str(),
                    Some("unavailable")
                );
                assert_eq!(
                    result_json["capabilities"]["template_match"]["reason"].as_str(),
                    Some("no_capability_handles")
                );
            }
            other => panic!("expected inspection success, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn prepared_vision_context_inspects_and_dry_runs_top_strip() {
        use rollshot_agent::tools::{DryRunTool, RegionFeaturesTool, Tool};

        let image = image::RgbaImage::from_fn(64, 64, |_, _| image::Rgba([160, 170, 180, 255]));
        let vision = prepare_vision_context(&image).unwrap();
        let catalog = canonical_region_feature_catalog(64, 64);
        let inspection = authoring_inspection_context(
            PayloadMode::FullScreenshot,
            &catalog,
            &canonical_ocr_catalog(64, 64),
        );
        let ctx = tool_context_for_tests();
        let host = vision.host.clone()
            as std::sync::Arc<std::sync::Mutex<dyn rollshot_automation::AutomationHost>>;

        let inspect =
            RegionFeaturesTool::new(ctx.clone(), host.clone(), inspection.regions.clone());
        let inspected = inspect
            .call(&serde_json::json!({"region": "top_strip"}))
            .await
            .unwrap();
        match inspected {
            rollshot_agent::tools::ToolOutcome::Success { result_json } => {
                assert_eq!(result_json["status"].as_str(), Some("available"));
                assert_eq!(result_json["features"].as_array().unwrap().len(), 1);
            }
            other => panic!("expected inspection success, got {other:?}"),
        }

        let source = r#"
function main(input) {
  const bounds = { x: 0, y: 0, width: input.imageWidth, height: Math.min(96, input.imageHeight) };
  const features = rollshot.regionFeatures({ region: { kind: "rect", bounds: bounds }, limit: 1 });
  return { candidates: features.length > 0 ? [{ kind: "addRedaction", bounds: bounds, confidence: 0.6, label: "top-strip" }] : [] };
}
"#;
        let dry_run = DryRunTool::new(
            ctx,
            std::sync::Arc::new(rollshot_automation_rquickjs::QuickJsExecutor),
            host,
        );
        let dry_run_result = dry_run
            .call(&serde_json::json!({"source": source, "generation": 0}))
            .await
            .unwrap();
        match dry_run_result {
            rollshot_agent::tools::ToolOutcome::Success { result_json } => {
                assert_eq!(result_json["candidate_count"].as_u64(), Some(1));
                assert_eq!(
                    result_json["candidate_preview"][0]["label"].as_str(),
                    Some("top-strip")
                );
            }
            other => panic!("expected dry-run success, got {other:?}"),
        }
    }

    #[cfg(feature = "ocr")]
    #[test]
    fn authoring_registry_exposes_ocr_tool_when_feature_enabled() {
        let ctx = tool_context_for_tests();
        let executor: std::sync::Arc<dyn rollshot_automation::AutomationExecutor> =
            std::sync::Arc::new(rollshot_automation_rquickjs::QuickJsExecutor);
        let host: std::sync::Arc<std::sync::Mutex<dyn rollshot_automation::AutomationHost>> =
            std::sync::Arc::new(std::sync::Mutex::new(
                rollshot_automation::FakeAutomationHost::default(),
            ));
        let inspection = authoring_inspection_context(
            PayloadMode::FullScreenshot,
            &canonical_region_feature_catalog(64, 64),
            &canonical_ocr_catalog(64, 64),
        );

        let registry = build_authoring_tool_registry(ctx, executor, host, inspection).unwrap();
        let names = registry.tool_names();

        assert!(names.contains(&"inspect_ocr"));
        assert!(!names.contains(&"inspect_layout"));
    }

    #[cfg(feature = "ocr")]
    #[tokio::test]
    async fn prepared_vision_context_dry_runs_full_ocr_query() {
        use imageproc::drawing::draw_text_mut;
        use rollshot_agent::tools::{DryRunTool, OcrTool, Tool};

        let font = ab_glyph::FontRef::try_from_slice(include_bytes!(
            "../../../../rollshot-image-document/assets/fonts/DejaVuSans.ttf"
        ));
        if font.is_err() {
            return;
        }
        let font = font.unwrap();
        let mut image = image::RgbaImage::from_pixel(480, 160, image::Rgba([255, 255, 255, 255]));
        draw_text_mut(
            &mut image,
            image::Rgba([0, 0, 0, 255]),
            20,
            40,
            ab_glyph::PxScale::from(32.0),
            &font,
            "alice@example.com",
        );

        let vision = prepare_vision_context(&image).unwrap();
        let region_catalog = canonical_region_feature_catalog(480, 160);
        let ocr_catalog = canonical_ocr_catalog(480, 160);
        let inspection = authoring_inspection_context(
            PayloadMode::FullScreenshot,
            &region_catalog,
            &ocr_catalog,
        );
        let cancel = rollshot_agent::runtime::RunCancellation::new();
        let ctx = std::sync::Arc::new(rollshot_agent::tools::ToolContext::new_with_capability_handles(
            rollshot_agent::domain::SessionId::new(1),
            String::new(),
            rollshot_automation::ValidationLimits::default(),
            rollshot_automation::ExecutionPolicy::smart_redaction_default(
                std::time::Duration::from_secs(5),
                4 * 1024 * 1024,
                1024 * 1024,
            ),
            (480, 160),
            product_capability_handles(),
            &cancel,
        ));
        let host = vision.host.clone()
            as std::sync::Arc<std::sync::Mutex<dyn rollshot_automation::AutomationHost>>;

        let inspect = OcrTool::new(ctx.clone(), host.clone(), inspection.ocr_regions.clone());
        let inspected = inspect
            .call(&serde_json::json!({"region": "full"}))
            .await
            .unwrap();
        match inspected {
            rollshot_agent::tools::ToolOutcome::Success { result_json } => {
                assert_eq!(result_json["status"].as_str(), Some("available"));
            }
            other => panic!("expected OCR inspection success, got {other:?}"),
        }

        let source = r#"
function main(input) {
  const matches = rollshot.ocr({ region: { kind: "full" }, limit: 50 });
  return {
    candidates: matches.map((match) => ({
      kind: "addRedaction",
      bounds: match.bounds,
      confidence: match.confidence,
      label: "ocr-match"
    }))
  };
}
"#;
        let dry_run = DryRunTool::new(
            ctx,
            std::sync::Arc::new(rollshot_automation_rquickjs::QuickJsExecutor),
            host,
        );
        let dry_run_result = dry_run
            .call(&serde_json::json!({"source": source, "generation": 0}))
            .await
            .unwrap();
        match dry_run_result {
            rollshot_agent::tools::ToolOutcome::Success { result_json } => {
                assert!(
                    result_json["candidate_count"].as_u64().unwrap_or(0) > 0,
                    "expected OCR dry-run candidates, got {result_json}"
                );
            }
            other => panic!("expected dry-run success, got {other:?}"),
        }
    }
}

#[cfg(test)]
mod reducer_tests {
    use crate::result_workspace::document::ResultDocument;
    use crate::result_workspace::update::{update, Message};
    use crate::result_workspace::workbench::{WorkbenchMessage, WorkbenchState, WorkspaceMode};
    use crate::result_workspace::ResultWorkspace;
    use rollshot_agent::driver::RunTerminalState;
    use rollshot_edit_proposal::{
        CandidateId, ConfidenceSummary, EditProposal, ProposalId, ProposedCandidate, ProposedEdit,
        Provenance, ProvenanceSource,
    };
    use rollshot_image_document::ImageRect;

    fn rect(x: f32, y: f32, w: f32, h: f32) -> ImageRect {
        ImageRect {
            x,
            y,
            width: w,
            height: h,
        }
    }

    fn candidate(id: u64, b: ImageRect) -> ProposedCandidate {
        ProposedCandidate {
            id: CandidateId(id),
            edit: ProposedEdit::AddRedaction { bounds: b },
            confidence: 0.9,
            label: "t".into(),
            rationale: None,
            provenance: Provenance {
                source: ProvenanceSource::Manual,
            },
        }
    }

    fn proposal(cands: Vec<ProposedCandidate>) -> EditProposal {
        EditProposal {
            id: ProposalId(1),
            base_document_state_id: 0,
            candidates: cands,
            confidence_summary: ConfidenceSummary::from_confidences(&[0.9]),
            rationale_summary: None,
            provenance: Provenance {
                source: ProvenanceSource::Manual,
            },
        }
    }

    fn ws_with_workbench() -> ResultWorkspace {
        let img = image::RgbaImage::new(200, 200);
        let mut ws = ResultWorkspace::new(ResultDocument::unsaved(img), None);
        ws.mode = WorkspaceMode::Workbench(WorkbenchState::default());
        ws
    }

    fn wb(ws: &ResultWorkspace) -> &WorkbenchState {
        match &ws.mode {
            WorkspaceMode::Workbench(wb) => wb,
            _ => panic!("expected workbench mode"),
        }
    }

    fn wb_mut(ws: &mut ResultWorkspace) -> &mut WorkbenchState {
        match &mut ws.mode {
            WorkspaceMode::Workbench(wb) => wb,
            _ => panic!("expected workbench mode"),
        }
    }

    #[test]
    fn run_terminal_ready_for_review_populates_proposal_review_draft() {
        use rollshot_agent::domain::SessionId;
        use rollshot_agent::driver::{DraftAutomation, DryRunEvidence, ReadyForReview};
        use rollshot_agent::runtime::UsageSnapshot;

        let mut ws = ws_with_workbench();
        let p = proposal(vec![
            candidate(1, rect(10.0, 10.0, 50.0, 50.0)),
            candidate(2, rect(100.0, 100.0, 30.0, 30.0)),
        ]);
        let ready = ReadyForReview {
            automation: DraftAutomation {
                source: "function main(input) { return { candidates: [] }; }".into(),
                validated: rollshot_automation::validate_source(
                    "function main(input) { return { candidates: [] }; }",
                    &rollshot_automation::ValidationLimits::default(),
                )
                .unwrap(),
                validation_summary: rollshot_automation::ValidationSummary {
                    source_bytes: 0,
                    ast_nodes: 0,
                    helper_count: 0,
                    capability_calls: 0,
                    max_output_candidates: 0,
                },
                dry_run: DryRunEvidence {
                    candidate_count: 2,
                    affected_area: 100.0,
                },
            },
            proposal: p.clone(),
            budget_usage: UsageSnapshot::default(),
            session_id: SessionId::new(0),
            assistant_text: "done".into(),
            generation: 1,
            usage: UsageSnapshot::default(),
        };
        let _ = update(
            &mut ws,
            Message::Workbench(WorkbenchMessage::RunTerminal(
                RunTerminalState::ReadyForReview(Box::new(ready)),
            )),
        );
        let state = wb(&ws);
        assert!(state.pending_proposal.is_some(), "proposal populated");
        assert_eq!(state.pending_proposal.as_ref().unwrap().candidates.len(), 2);
        assert_eq!(state.review.per_candidate.len(), 2);
        assert!(state.pending_draft.is_some(), "draft populated");
        assert_eq!(state.pending_draft.as_ref().unwrap().assistant_text, "done");
        assert!(matches!(
            state.run_state,
            super::super::RunState::Terminal(_)
        ));
    }

    #[test]
    fn apply_candidates_commits_and_clears_proposal() {
        let mut ws = ws_with_workbench();
        let p = proposal(vec![candidate(1, rect(10.0, 10.0, 50.0, 50.0))]);
        let review = super::super::CandidateReview::from_candidates(&[CandidateId(1)]);
        wb_mut(&mut ws).pending_proposal = Some(p);
        wb_mut(&mut ws).review = review;

        let _ = update(
            &mut ws,
            Message::Workbench(WorkbenchMessage::ApplyCandidates),
        );
        let state = wb(&ws);
        assert!(state.pending_proposal.is_none(), "proposal cleared");
        assert!(state.review.is_empty(), "review cleared");
        assert_eq!(
            ws.document.image.annotations().len(),
            1,
            "annotation committed"
        );
    }

    #[test]
    fn candidate_deleted_marks_rejected() {
        let mut ws = ws_with_workbench();
        let review =
            super::super::CandidateReview::from_candidates(&[CandidateId(1), CandidateId(2)]);
        wb_mut(&mut ws).review = review;
        wb_mut(&mut ws).selected_candidate = Some(CandidateId(1));

        let _ = update(
            &mut ws,
            Message::Workbench(WorkbenchMessage::CandidateDeleted(CandidateId(1))),
        );
        let state = wb(&ws);
        assert_eq!(
            state.review.per_candidate[&CandidateId(1)],
            super::super::CandidateReviewState::Rejected,
        );
        assert!(
            state.selected_candidate.is_none(),
            "selection cleared when deleted"
        );
    }

    #[test]
    fn candidate_unrejected_returns_to_pending() {
        let mut ws = ws_with_workbench();
        let mut review = super::super::CandidateReview::from_candidates(&[CandidateId(1)]);
        review.mark_rejected(CandidateId(1));
        wb_mut(&mut ws).review = review;

        let _ = update(
            &mut ws,
            Message::Workbench(WorkbenchMessage::CandidateUnrejected(CandidateId(1))),
        );
        let state = wb(&ws);
        assert_eq!(
            state.review.per_candidate[&CandidateId(1)],
            super::super::CandidateReviewState::Pending,
        );
    }

    #[test]
    fn send_requested_captures_run_params_and_sets_disclosure() {
        let mut ws = ws_with_workbench();
        wb_mut(&mut ws).composer = "test message".into();

        let _ = update(&mut ws, Message::Workbench(WorkbenchMessage::SendRequested));
        let state = wb(&ws);
        assert!(state.disclosure_pending, "disclosure opened");
        let params = state.pending_run.as_ref().unwrap();
        assert_eq!(params.user_message, "test message");
        assert!(state.composer.is_empty(), "composer cleared");
    }

    #[test]
    fn send_requested_noop_when_composer_empty() {
        let mut ws = ws_with_workbench();
        wb_mut(&mut ws).composer = String::new();

        let _ = update(&mut ws, Message::Workbench(WorkbenchMessage::SendRequested));
        let state = wb(&ws);
        assert!(!state.disclosure_pending);
        assert!(state.pending_run.is_none());
    }

    #[test]
    fn disclosure_cancelled_clears_pending_run_and_flag() {
        let mut ws = ws_with_workbench();
        wb_mut(&mut ws).disclosure_pending = true;
        wb_mut(&mut ws).pending_run = Some(super::super::PendingRunParams {
            user_message: "test".into(),
            image_dims: (100, 100),
            active_revision_source: None,
            mode: super::super::RunKind::Author,
        });

        let _ = update(
            &mut ws,
            Message::Workbench(WorkbenchMessage::DisclosureCancelled),
        );
        let state = wb(&ws);
        assert!(!state.disclosure_pending);
        assert!(state.pending_run.is_none());
    }

    #[test]
    fn run_event_pushes_activity_entry() {
        use rollshot_agent::runtime::RunEvent;

        let mut ws = ws_with_workbench();
        let _ = update(
            &mut ws,
            Message::Workbench(WorkbenchMessage::RunEvent(RunEvent::TextChunk {
                text: "hello".into(),
            })),
        );
        let state = wb(&ws);
        assert_eq!(state.live_activity.len(), 1);
    }

    #[test]
    fn text_chunks_accumulate_into_one_entry() {
        use rollshot_agent::runtime::RunEvent;

        let mut ws = ws_with_workbench();
        let _ = update(
            &mut ws,
            Message::Workbench(WorkbenchMessage::RunEvent(RunEvent::TextChunk {
                text: "hello ".into(),
            })),
        );
        let _ = update(
            &mut ws,
            Message::Workbench(WorkbenchMessage::RunEvent(RunEvent::TextChunk {
                text: "world".into(),
            })),
        );
        let state = wb(&ws);
        assert_eq!(state.live_activity.len(), 1, "two chunks → one entry");
        match &state.live_activity[0] {
            super::super::state::ActivityEntry::AssistantText(t) => {
                assert_eq!(t, "hello world");
            }
            other => panic!("expected AssistantText, got {other:?}"),
        }
    }

    #[test]
    fn terminal_reconciles_assistant_text() {
        use rollshot_agent::runtime::RunEvent;

        let mut ws = ws_with_workbench();
        // Streamed chunks (may have gaps from dropped try_send).
        let _ = update(
            &mut ws,
            Message::Workbench(WorkbenchMessage::RunEvent(RunEvent::TextChunk {
                text: "hel".into(),
            })),
        );
        let _ = update(
            &mut ws,
            Message::Workbench(WorkbenchMessage::RunEvent(RunEvent::TextChunk {
                text: "lo".into(),
            })),
        );
        // Terminal with authoritative full text.
        let ready = ready_for_review_with_text("hello world");
        let _ = update(
            &mut ws,
            Message::Workbench(WorkbenchMessage::RunTerminal(
                RunTerminalState::ReadyForReview(Box::new(ready)),
            )),
        );
        let state = wb(&ws);
        // Find the AssistantText entry (before the TerminalLabel).
        let assistant_text = state.live_activity.iter().find_map(|e| match e {
            super::super::state::ActivityEntry::AssistantText(t) => Some(t.as_str()),
            _ => None,
        });
        assert_eq!(
            assistant_text,
            Some("hello world"),
            "reconciled to authoritative text"
        );
    }

    fn ready_for_review_with_text(text: &str) -> rollshot_agent::driver::ReadyForReview {
        use rollshot_agent::domain::SessionId;
        use rollshot_agent::driver::{DraftAutomation, DryRunEvidence, ReadyForReview};
        use rollshot_agent::runtime::UsageSnapshot;
        ReadyForReview {
            automation: DraftAutomation {
                source: "function main(input) { return { candidates: [] }; }".into(),
                validated: rollshot_automation::validate_source(
                    "function main(input) { return { candidates: [] }; }",
                    &rollshot_automation::ValidationLimits::default(),
                )
                .unwrap(),
                validation_summary: rollshot_automation::ValidationSummary {
                    source_bytes: 0,
                    ast_nodes: 0,
                    helper_count: 0,
                    capability_calls: 0,
                    max_output_candidates: 0,
                },
                dry_run: DryRunEvidence {
                    candidate_count: 0,
                    affected_area: 0.0,
                },
            },
            proposal: rollshot_edit_proposal::EditProposal {
                id: rollshot_edit_proposal::ProposalId(1),
                base_document_state_id: 0,
                candidates: vec![],
                confidence_summary: rollshot_edit_proposal::ConfidenceSummary::from_confidences(&[]),
                rationale_summary: None,
                provenance: rollshot_edit_proposal::Provenance {
                    source: rollshot_edit_proposal::ProvenanceSource::Manual,
                },
            },
            budget_usage: UsageSnapshot::default(),
            session_id: SessionId::new(0),
            assistant_text: text.into(),
            generation: 1,
            usage: UsageSnapshot::default(),
        }
    }

    #[test]
    fn cancel_run_calls_cancellation() {
        use rollshot_agent::runtime::RunCancellation;

        let mut ws = ws_with_workbench();
        let cancel = RunCancellation::new();
        wb_mut(&mut ws).run_state = super::super::RunState::Running {
            cancellation: cancel.clone(),
        };

        let _ = update(&mut ws, Message::Workbench(WorkbenchMessage::CancelRun));
        assert!(cancel.is_cancelled());
    }

    #[test]
    fn run_failed_sets_error_and_terminal() {
        let mut ws = ws_with_workbench();
        wb_mut(&mut ws).run_state = super::super::RunState::Running {
            cancellation: rollshot_agent::runtime::RunCancellation::new(),
        };

        let _ = update(
            &mut ws,
            Message::Workbench(WorkbenchMessage::RunFailed(
                super::WorkbenchError::VisionPrepare {
                    message: "region_too_large".into(),
                },
            )),
        );
        let state = wb(&ws);
        assert!(
            matches!(
                &state.error,
                Some(super::WorkbenchError::VisionPrepare { message }) if message == "region_too_large"
            ),
            "typed error preserved"
        );
        assert!(
            matches!(state.run_state, super::super::RunState::Terminal(_)),
            "run transitioned to terminal"
        );
    }

    #[test]
    fn disclosure_confirmed_blocked_while_running() {
        let mut ws = ws_with_workbench();
        wb_mut(&mut ws).run_state = super::super::RunState::Running {
            cancellation: rollshot_agent::runtime::RunCancellation::new(),
        };
        wb_mut(&mut ws).disclosure_pending = true;
        wb_mut(&mut ws).pending_run = Some(super::super::PendingRunParams {
            user_message: "test".into(),
            image_dims: (100, 100),
            active_revision_source: None,
            mode: super::super::RunKind::Author,
        });

        let _ = update(
            &mut ws,
            Message::Workbench(WorkbenchMessage::DisclosureConfirmed),
        );
        let state = wb(&ws);
        // The guard bails before consuming pending_run.
        assert!(
            state.pending_run.is_some(),
            "pending_run not consumed when running"
        );
        assert!(
            state.run_state.is_running(),
            "run_state unchanged (no second run started)"
        );
    }
}
