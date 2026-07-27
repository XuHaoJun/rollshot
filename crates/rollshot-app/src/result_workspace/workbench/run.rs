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

const PHASE_F_TEMPLATE_MATCH_LIMIT: u32 = 8;

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct CanonicalOcrEntry {
    pub(crate) name: &'static str,
    pub(crate) bounds: rollshot_image_document::ImageRect,
    pub(crate) query: Option<rollshot_automation::OcrQuery>,
    pub(crate) unavailable_reason: Option<&'static str>,
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
pub(crate) struct CanonicalRegionFeatureEntry {
    pub(crate) name: &'static str,
    pub(crate) bounds: rollshot_image_document::ImageRect,
    pub(crate) query: Option<rollshot_automation::RegionFeaturesQuery>,
    pub(crate) unavailable_reason: Option<&'static str>,
}

pub(crate) fn canonical_region_feature_catalog(
    width: u32,
    height: u32,
) -> Vec<CanonicalRegionFeatureEntry> {
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

pub(crate) fn canonical_ocr_catalog(width: u32, height: u32) -> Vec<CanonicalOcrEntry> {
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

// Phase F v1: dormant until template-creation UI adds preset-local template handles
pub(crate) fn product_capability_handles() -> std::collections::BTreeMap<String, String> {
    std::collections::BTreeMap::new()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CapabilityAvailability {
    pub available: bool,
    pub reason: Option<String>,
}

impl CapabilityAvailability {
    fn available() -> Self {
        Self {
            available: true,
            reason: None,
        }
    }

    fn unavailable(reason: &str) -> Self {
        Self {
            available: false,
            reason: Some(reason.into()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ProductCapabilityAvailability {
    pub template_match: CapabilityAvailability,
}

#[derive(Debug)]
pub(crate) struct ProductCapabilityBundle {
    pub capability_handles: std::collections::BTreeMap<String, String>,
    pub template_store: rollshot_vision::TemplateStore,
    pub template_summaries: Vec<rollshot_vision::TemplateAssetSummary>,
    pub availability: ProductCapabilityAvailability,
}

impl ProductCapabilityBundle {
    pub(crate) fn empty() -> Self {
        Self {
            capability_handles: std::collections::BTreeMap::new(),
            template_store: rollshot_vision::TemplateStore::new(),
            template_summaries: Vec::new(),
            availability: ProductCapabilityAvailability {
                template_match: CapabilityAvailability::unavailable("no_capability_handles"),
            },
        }
    }

    #[cfg(test)]
    pub(crate) fn from_template_store_for_tests(
        template_store: rollshot_vision::TemplateStore,
    ) -> Self {
        let template_summaries = template_store.summaries();
        let capability_handles = template_summaries
            .iter()
            .map(|summary| (summary.handle.clone(), summary.handle.clone()))
            .collect();
        let template_match = if template_summaries.is_empty() {
            CapabilityAvailability::unavailable("no_capability_handles")
        } else {
            CapabilityAvailability::available()
        };
        Self {
            capability_handles,
            template_store,
            template_summaries,
            availability: ProductCapabilityAvailability { template_match },
        }
    }

    pub(crate) fn load(
        store: &rollshot_preset::PresetStore,
        preset_id: Option<&rollshot_preset::PresetId>,
    ) -> Result<Self, WorkbenchError> {
        let Some(preset_id) = preset_id else {
            return Ok(Self::empty());
        };
        let path = store
            .template_store_path(preset_id)
            .map_err(|_| WorkbenchError::RuntimeFailure)?;
        if !path.exists() {
            return Ok(Self::empty());
        }
        let template_store = rollshot_vision::TemplateStore::load_local(&path).map_err(|e| {
            WorkbenchError::VisionPrepare {
                message: format!("template store: {e}"),
            }
        })?;
        let template_summaries = template_store.summaries();
        let capability_handles = template_summaries
            .iter()
            .map(|summary| (summary.handle.clone(), summary.handle.clone()))
            .collect();
        let template_match = if template_summaries.is_empty() {
            CapabilityAvailability::unavailable("no_capability_handles")
        } else {
            CapabilityAvailability::available()
        };
        Ok(Self {
            capability_handles,
            template_store,
            template_summaries,
            availability: ProductCapabilityAvailability { template_match },
        })
    }
}

pub(crate) fn authoring_inspection_context(
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

pub(crate) fn revision_capability_metadata(
    validated: &rollshot_automation::ValidatedAutomation,
    bundle: &ProductCapabilityBundle,
) -> rollshot_preset::RevisionCapabilityMetadata {
    let mut requirements = Vec::new();
    for call in &validated.workflow_ir.capability_manifest.calls {
        let exists =
            requirements
                .iter()
                .any(|r: &rollshot_preset::RevisionCapabilityRequirement| {
                    r.capability == call.capability && r.alias.is_none()
                });
        if !exists {
            requirements.push(rollshot_preset::RevisionCapabilityRequirement {
                capability: call.capability,
                alias: None,
                required: true,
            });
        }
    }
    let template_handles = bundle
        .template_summaries
        .iter()
        .map(|summary| rollshot_preset::TemplateHandleMetadata {
            alias: summary.handle.clone(),
            handle: summary.handle.clone(),
            display_name: summary.handle.clone(),
            sensitivity_sensitive: matches!(
                summary.sensitivity,
                rollshot_vision::TemplateSensitivity::Sensitive
            ),
            source_agent_suggested: matches!(
                summary.source,
                rollshot_vision::TemplateSource::AgentSuggested
            ),
        })
        .collect();
    rollshot_preset::RevisionCapabilityMetadata {
        requirements,
        template_handles,
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

fn canonical_template_queries(
    image_width: u32,
    image_height: u32,
    handles: &std::collections::BTreeMap<String, String>,
) -> Vec<rollshot_automation::TemplateMatchQuery> {
    let regions = canonical_region_feature_catalog(image_width, image_height);
    handles
        .values()
        .flat_map(|handle| {
            regions.iter().filter_map(move |entry| {
                entry
                    .query
                    .as_ref()
                    .map(|region_query| rollshot_automation::TemplateMatchQuery {
                        template_handle: handle.clone(),
                        region: region_query.region,
                        limit: PHASE_F_TEMPLATE_MATCH_LIMIT,
                    })
            })
        })
        .collect()
}

fn prepare_phase_f_templates(
    host: &mut rollshot_vision::RealAutomationHost,
    index: &VisualIndex,
    bundle: &ProductCapabilityBundle,
) -> Result<(), WorkbenchError> {
    for query in
        canonical_template_queries(index.width(), index.height(), &bundle.capability_handles)
    {
        match host.prepare_template_match(index, &bundle.template_store, &query) {
            Ok(()) => {}
            Err(rollshot_automation::CapabilityError::InvalidInput { code })
                if matches!(
                    code,
                    "region_too_large" | "template_larger_than_region" | "template_low_information"
                ) =>
            {
                tracing::debug!(
                    target: "rollshot::vision::template",
                    template_handle = %query.template_handle,
                    code,
                    "skipped infeasible template preparation"
                );
            }
            Err(e) => {
                return Err(WorkbenchError::VisionPrepare {
                    message: format!("templateMatch {}: {e}", query.template_handle),
                });
            }
        }
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
    run_existing_preset_with_capabilities(
        image,
        revision,
        policy,
        &ProductCapabilityBundle::empty(),
    )
}

fn revision_requires_template_match(revision: &AutomationRevision) -> bool {
    revision
        .artifact
        .workflow_ir
        .capability_manifest
        .calls
        .iter()
        .any(|call| call.capability == rollshot_automation::CapabilityName::TemplateMatch)
}

pub(crate) fn run_existing_preset_with_capabilities(
    image: &image::RgbaImage,
    revision: &AutomationRevision,
    policy: &ExecutionPolicy,
    bundle: &ProductCapabilityBundle,
) -> Result<EditProposal, WorkbenchError> {
    let (w, h) = image.dimensions();
    let index = VisualIndex::build(image.clone()).map_err(|e| WorkbenchError::VisionPrepare {
        message: format!("VisualIndex: {e}"),
    })?;
    if revision_requires_template_match(revision) && bundle.capability_handles.is_empty() {
        return Err(WorkbenchError::CapabilityUnavailable {
            message: "This preset uses template matching, but no template handles are available for this preset.".into(),
        });
    }
    let mut host = rollshot_vision::RealAutomationHost::new();
    prepare_phase_a_region_features(&mut host, &index)?;
    #[cfg(feature = "ocr")]
    prepare_phase_b2_ocr(&mut host, &index)?;
    prepare_phase_f_templates(&mut host, &index, bundle)?;
    let executor = QuickJsExecutor;
    let cancellation = CancellationFlag::default();
    let input = AutomationInput {
        image_width: w,
        image_height: h,
        region: None,
        annotations: vec![],
        capability_handles: bundle.capability_handles.clone(),
    };
    let ctx = ProposalContext {
        proposal_id: ProposalId::parse("proposal-00000001-0000-4000-8000-000000000000").unwrap(),
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
    bundle: &ProductCapabilityBundle,
) -> Result<super::VisionContext, WorkbenchError> {
    let index = VisualIndex::build(image.clone()).map_err(|e| WorkbenchError::VisionPrepare {
        message: format!("VisualIndex: {e}"),
    })?;
    let mut host = rollshot_vision::RealAutomationHost::new();
    prepare_phase_a_region_features(&mut host, &index)?;
    #[cfg(feature = "ocr")]
    prepare_phase_b2_ocr(&mut host, &index)?;
    prepare_phase_f_templates(&mut host, &index, bundle)?;
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

pub(crate) fn build_authoring_tool_registry(
    tool_ctx: Arc<rollshot_agent::tools::ToolContext>,
    executor: Arc<dyn rollshot_automation::AutomationExecutor>,
    host: Arc<StdMutex<dyn rollshot_automation::AutomationHost>>,
    inspection: rollshot_agent::tools::AuthoringInspectionContext,
) -> Result<rollshot_agent::tools::ToolRegistry, WorkbenchError> {
    #[cfg(feature = "ocr")]
    use rollshot_agent::tools::OcrTool;
    use rollshot_agent::tools::{
        DryRunTool, EditSourceTool, GetContextSummaryTool, InspectImageContextTool,
        ReadCurrentSourceTool, RegionFeaturesTool, ReplaceSourceTool, RequestUserInputTool,
        SubmitForReviewTool, ToolRegistry, ToolRegistryLimits, ValidateSourceTool,
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
        Arc::new(ReadCurrentSourceTool::new(tool_ctx.clone())),
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
        Arc::new(EditSourceTool::new(tool_ctx.clone())),
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

// ── Persistence-before-delivery helpers ────────────────────────────────

/// Persist a terminal snapshot for a setup failure.  Called before yielding
/// `RunFailed` to iced so the store always reflects the terminal state.
/// Returns silently on success or store error (the store error is not
/// surfaced to the user — the `RunFailed` message carries the real cause).
async fn persist_terminal_if_possible(
    task_store: Option<Arc<super::task_store::TaskStore>>,
    task_id: &rollshot_agent::product_task::ProductTaskId,
    run_id: &rollshot_agent::domain::RunId,
    error: &WorkbenchError,
) {
    let Some(store) = task_store else { return };
    use rollshot_agent::product_task::{
        ProductTaskSnapshot, SourceBinding, TaskAttempt, TaskAttemptId, TaskKind,
    };
    let now = chrono::Utc::now().timestamp_millis();
    // Minimal snapshot just to record the terminal state.
    let source_binding = SourceBinding::new(
        [0u8; 32], [0u8; 32], 0, String::new(), None,
    );
    let snapshot = match ProductTaskSnapshot::new(
        task_id.clone(), TaskKind::SmartRedactionAuthor, source_binding, now,
    ) {
        Ok(s) => s,
        Err(_) => return,
    };
    let attempt = TaskAttempt::new(TaskAttemptId::new(1), run_id.clone(), now);
    let running = match snapshot.start_attempt(attempt, now) {
        Ok(s) => s,
        Err(_) => return,
    };
    let terminal = error.to_task_terminal();
    let terminal_snap = match running.record_terminal(terminal, now) {
        Ok(s) => s,
        Err(_) => return,
    };
    let store = store.clone();
    let _ = tokio::task::spawn_blocking(move || {
        let _ = store.compare_and_swap(&running, &terminal_snap);
    })
    .await;
}

/// Persist a `RunTerminal` outcome (artifact for `ReadyForReview`, terminal
/// for others) before yielding to iced.  Returns `Some(err_msg)` if the store
/// failed — caller should yield a bounded `StorePersist` error and suppress
/// the proposal.
async fn persist_terminal_outcome(
    task_store: Option<Arc<super::task_store::TaskStore>>,
    task_id: &rollshot_agent::product_task::ProductTaskId,
    _run_id: &rollshot_agent::domain::RunId,
    proposal_id_str: &str,
    content_binding: &rollshot_agent::product_task::DocumentContentBinding,
    terminal: &rollshot_agent::driver::RunTerminalState,
) -> Option<String> {
    use rollshot_agent::driver::RunTerminalState;
    use rollshot_agent::product_task::{
        ArtifactId, ArtifactKind, ArtifactRevision, ProductArtifactMetadata,
        SourceBinding, TaskTerminal,
    };

    let Some(store) = task_store else { return None };

    // Load current snapshot from store (must exist — we persisted running).
    let current = match store.load(task_id) {
        Ok(s) => s,
        Err(e) => return Some(format!("load for terminal: {e}")),
    };

    let now = chrono::Utc::now().timestamp_millis();

    let next = match terminal {
        RunTerminalState::ReadyForReview(ready) => {
            let source_binding = SourceBinding::new(
                *content_binding.base_image_digest(),
                *content_binding.annotation_state_digest(),
                content_binding.state_id(),
                current.source_binding().preset_id().to_owned(),
                current.source_binding().active_preset_revision_id().map(String::from),
            );
            let artifact_id = ArtifactId::parse(format!(
                "artifact-{}",
                uuid::Uuid::new_v4()
            ))
            .expect("v4 UUID is valid");
            let metadata = ProductArtifactMetadata::new(
                artifact_id,
                ArtifactRevision::new(1),
                ArtifactKind::SmartRedaction,
                1,
                String::new(), // canonical payload digest — computed on apply
                source_binding,
                task_id.clone(),
                current.attempts().last().unwrap().attempt_id(),
                current.attempts().last().unwrap().run_id().clone(),
                proposal_id_str.to_owned(),
                String::new(), // provider_id — not available here
                String::new(), // model_id — not available here
                String::new(), // run_config_digest
                ready.automation.dry_run.candidate_count,
                ready.automation.dry_run.affected_area,
                now,
            );
            // Build a minimal SmartRedactionReviewPayload for persistence.
            let payload = rollshot_agent::product_task::SmartRedactionReviewPayload {
                source: rollshot_agent::product_task::PayloadSourceV1 {
                    kind: "agent_run".into(),
                    validation_summary: format!(
                        "{} nodes",
                        ready.automation.validation_summary.ast_nodes
                    ),
                },
                proposal: rollshot_agent::product_task::PayloadProposalV1 {
                    proposal_id: proposal_id_str.to_owned(),
                    candidate_count: ready.proposal.candidates.len() as u32,
                },
                dry_run: rollshot_agent::product_task::PayloadDryRunV1 {
                    candidate_count: ready.automation.dry_run.candidate_count,
                    affected_area: ready.automation.dry_run.affected_area,
                },
                config: rollshot_agent::product_task::PayloadConfigV1 {
                    provider: String::new(),
                    model: String::new(),
                    payload_mode: rollshot_agent::product_task::PayloadMode::Author,
                    run_kind: "smart_redaction".into(),
                    budget_dimensions: std::collections::BTreeMap::new(),
                },
            };
            match current.record_ready_for_review(metadata, payload, now) {
                Ok(s) => s,
                Err(e) => return Some(format!("record ready: {e}")),
            }
        }
        other => {
            let task_terminal = match other {
                RunTerminalState::NeedsUserInput(_) => TaskTerminal::NeedsUserInput,
                RunTerminalState::Cancelled => TaskTerminal::Cancelled,
                RunTerminalState::BudgetExhausted { dimension } => TaskTerminal::BudgetExhausted {
                    dimension: format!("{dimension:?}"),
                },
                RunTerminalState::SourceValidationFailure => TaskTerminal::SourceValidationFailure,
                RunTerminalState::RuntimeFailure => TaskTerminal::RuntimeFailure,
                RunTerminalState::AgentProtocolFailure { .. } => {
                    TaskTerminal::AgentProtocolFailure
                }
                RunTerminalState::ProviderFailure { .. } => TaskTerminal::ProviderFailure,
                RunTerminalState::ReadyForReview(_) => unreachable!(),
            };
            match current.record_terminal(task_terminal, now) {
                Ok(s) => s,
                Err(e) => return Some(format!("record terminal: {e}")),
            }
        }
    };

    let store = store.clone();
    let expected = current.clone();
    let result = tokio::task::spawn_blocking(move || {
        store.compare_and_swap(&expected, &next)
    })
    .await;
    match result {
        Ok(Ok(_)) => None,
        Ok(Err(e)) => Some(format!("persist terminal: {e}")),
        Err(e) => Some(format!("spawn_blocking: {e}")),
    }
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
    task_store: Option<Arc<super::task_store::TaskStore>>,
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
    let preset_store_root = params.preset_store_root.clone();
    let preset_id = params.preset_id.clone();
    let task_id = params.task_id.clone();
    let run_id = params.run_id.clone();
    let proposal_id = params.proposal_id.clone();
    let content_binding = params.content_binding.clone();
    let task_kind = match params.mode {
        super::RunKind::Author => rollshot_agent::product_task::TaskKind::SmartRedactionAuthor,
        super::RunKind::Improve => rollshot_agent::product_task::TaskKind::SmartRedactionImprove,
    };
    let task_store = task_store.clone();
    let image = image.clone();
    let budget = budget.clone();

    let cancellation = RunCancellation::new();
    let cancellation_for_task = cancellation.clone();

    let stream = async_stream::stream! {
        // ── Persistence-before-delivery protocol ──────────────────────
        // Step 1: persist running snapshot before any setup work.
        if let Some(ref store) = task_store {
            use rollshot_agent::product_task::{
                ProductTaskSnapshot, SourceBinding, TaskAttempt, TaskAttemptId,
            };
            let now = chrono::Utc::now().timestamp_millis();
            let source_binding = SourceBinding::new(
                *content_binding.base_image_digest(),
                *content_binding.annotation_state_digest(),
                content_binding.state_id(),
                preset_id.0.clone(),
                if active_source.is_empty() { None } else { Some(active_source.clone()) },
            );
            let attempt = TaskAttempt::new(
                TaskAttemptId::new(1),
                run_id.clone(),
                now,
            );
            let snapshot = match ProductTaskSnapshot::new(
                task_id.clone(),
                task_kind,
                source_binding,
                now,
            ) {
                Ok(s) => s,
                Err(e) => {
                    yield crate::result_workspace::Message::Workbench(
                        super::WorkbenchMessage::RunFailed {
                            task_id: task_id.clone(),
                            run_id: run_id.clone(),
                            error: WorkbenchError::StorePersist {
                                message: format!("create snapshot: {e}"),
                            },
                        },
                    );
                    return;
                }
            };
            let running = match snapshot.start_attempt(attempt, now) {
                Ok(s) => s,
                Err(e) => {
                    yield crate::result_workspace::Message::Workbench(
                        super::WorkbenchMessage::RunFailed {
                            task_id: task_id.clone(),
                            run_id: run_id.clone(),
                            error: WorkbenchError::StorePersist {
                                message: format!("start attempt: {e}"),
                            },
                        },
                    );
                    return;
                }
            };
            let store_clone = store.clone();
            let running_clone = running.clone();
            let persist_result = tokio::task::spawn_blocking(move || {
                store_clone.create(&running_clone)
            }).await;
            match persist_result {
                Ok(Ok(_)) => {} // running snapshot persisted
                Ok(Err(e)) => {
                    yield crate::result_workspace::Message::Workbench(
                        super::WorkbenchMessage::RunFailed {
                            task_id: task_id.clone(),
                            run_id: run_id.clone(),
                            error: WorkbenchError::StorePersist {
                                message: format!("persist running: {e}"),
                            },
                        },
                    );
                    return;
                }
                Err(e) => {
                    yield crate::result_workspace::Message::Workbench(
                        super::WorkbenchMessage::RunFailed {
                            task_id: task_id.clone(),
                            run_id: run_id.clone(),
                            error: WorkbenchError::StorePersist {
                                message: format!("spawn_blocking: {e}"),
                            },
                        },
                    );
                    return;
                }
            }
        }

        // Heavy work runs inside the spawned task (B5).
        let preset_store = rollshot_preset::PresetStore::open(preset_store_root);
        let capability_bundle = match ProductCapabilityBundle::load(&preset_store, Some(&preset_id)) {
            Ok(bundle) => bundle,
            Err(e) => {
                // Persist terminal before yielding setup failure.
                persist_terminal_if_possible(
                    task_store.clone(), &task_id, &run_id, &e,
                ).await;
                yield crate::result_workspace::Message::Workbench(
                    super::WorkbenchMessage::RunFailed {
                        task_id: task_id.clone(),
                        run_id: run_id.clone(),
                        error: e,
                    },
                );
                return;
            }
        };
        let vision = match prepare_vision_context(&image, &capability_bundle) {
            Ok(v) => v,
            Err(e) => {
                persist_terminal_if_possible(
                    task_store.clone(), &task_id, &run_id, &e,
                ).await;
                yield crate::result_workspace::Message::Workbench(
                    super::WorkbenchMessage::RunFailed {
                        task_id: task_id.clone(),
                        run_id: run_id.clone(),
                        error: e,
                    },
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
            session.run_id.clone(),
            proposal_id.clone(),
            content_binding.clone(),
            active_source,
            validation_limits,
            policy,
            image_dims,
            capability_bundle.capability_handles.clone(),
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
                persist_terminal_if_possible(
                    task_store.clone(), &task_id, &run_id, &e,
                ).await;
                yield crate::result_workspace::Message::Workbench(
                    super::WorkbenchMessage::RunFailed {
                        task_id: task_id.clone(),
                        run_id: run_id.clone(),
                        error: e,
                    },
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
                    let err = WorkbenchError::VisionPrepare {
                        message: format!("png encode: {e}"),
                    };
                    persist_terminal_if_possible(
                        task_store.clone(), &task_id, &run_id, &err,
                    ).await;
                    yield crate::result_workspace::Message::Workbench(
                        super::WorkbenchMessage::RunFailed {
                            task_id: task_id.clone(),
                            run_id: run_id.clone(),
                            error: err,
                        },
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
                let err = WorkbenchError::RuntimeFailure;
                persist_terminal_if_possible(
                    task_store.clone(), &task_id, &run_id, &err,
                ).await;
                yield crate::result_workspace::Message::Workbench(
                    super::WorkbenchMessage::RunFailed {
                        task_id: task_id.clone(),
                        run_id: run_id.clone(),
                        error: err,
                    },
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
                super::WorkbenchMessage::RunEvent {
                    task_id: task_id.clone(),
                    run_id: run_id.clone(),
                    event,
                },
            );
        }
        if let Ok(terminal) = run_task.await {
            // Persist terminal/artifact before yielding to iced.
            let persist_err = persist_terminal_outcome(
                task_store.clone(), &task_id, &run_id, proposal_id.as_str(),
                &content_binding, &terminal,
            ).await;
            if let Some(err_msg) = persist_err {
                // Store failure: yield bounded warning, do NOT yield proposal.
                yield crate::result_workspace::Message::Workbench(
                    super::WorkbenchMessage::RunFailed {
                        task_id: task_id.clone(),
                        run_id: run_id.clone(),
                        error: WorkbenchError::StorePersist { message: err_msg },
                    },
                );
                return;
            }
            yield crate::result_workspace::Message::Workbench(
                super::WorkbenchMessage::RunTerminal {
                    task_id: task_id.clone(),
                    run_id: run_id.clone(),
                    terminal,
                },
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
            proposal_id: ProposalId::parse("proposal-00000001-0000-4000-8000-000000000000")
                .unwrap(),
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
            capabilities: rollshot_preset::RevisionCapabilityMetadata::default(),
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
            capabilities: rollshot_preset::RevisionCapabilityMetadata::default(),
        }
    }

    #[test]
    fn run_existing_preset_prepares_template_handles_from_bundle() {
        let mut image = image::RgbaImage::from_fn(80, 80, |x, y| {
            let v = 120 + ((x * 3 + y * 5) % 23) as u8;
            image::Rgba([v, v, v, 255])
        });
        for y in 0..8 {
            for x in 0..8 {
                let v = ((x * 17 + y * 31 + x * y * 3) % 255) as u8;
                image.put_pixel(20 + x, 24 + y, image::Rgba([v, v, v, 255]));
            }
        }
        let tpl = image::imageops::crop_imm(&image, 20, 24, 8, 8).to_image();
        let mut store = rollshot_vision::TemplateStore::new();
        store
            .insert(rollshot_vision::TemplateAsset {
                handle: "mark".into(),
                sensitivity: rollshot_vision::TemplateSensitivity::Chrome,
                source: rollshot_vision::TemplateSource::UserRect,
                created_at_ms: 1,
                bounds_in_source_image: None,
                bytes: rollshot_vision::TemplateBytes::new(8, 8, tpl.into_raw()).unwrap(),
            })
            .unwrap();
        let bundle = ProductCapabilityBundle::from_template_store_for_tests(store);
        let source = r#"
function main(input) {
  const matches = rollshot.templateMatch({
    templateHandle: input.capabilityHandles.mark,
    region: { kind: "full" },
    limit: 1
  });
  return {
    candidates: matches.map((match) => ({
      kind: "addRedaction",
      bounds: match.bounds,
      confidence: match.score,
      label: "mark"
    }))
  };
}
"#;
        let revision = make_revision_from_source(source);
        let policy = ExecutionPolicy::smart_redaction_default(
            std::time::Duration::from_secs(10),
            100_000_000,
            8_000_000,
        );

        let proposal =
            run_existing_preset_with_capabilities(&image, &revision, &policy, &bundle).unwrap();

        assert_eq!(proposal.candidates.len(), 1);
    }

    #[test]
    fn template_using_existing_preset_without_handles_reports_capability_unavailable() {
        let source = r#"
function main(input) {
  const matches = rollshot.templateMatch({
    templateHandle: input.capabilityHandles.mark,
    region: { kind: "full" },
    limit: 1
  });
  return { candidates: matches.map((match) => ({
    kind: "addRedaction",
    bounds: match.bounds,
    confidence: match.score,
    label: "mark"
  })) };
}
"#;
        let image = image::RgbaImage::from_pixel(80, 80, image::Rgba([120, 120, 120, 255]));
        let revision = make_revision_from_source(source);
        let policy = ExecutionPolicy::smart_redaction_default(
            std::time::Duration::from_secs(10),
            100_000_000,
            8_000_000,
        );

        let err = run_existing_preset_with_capabilities(
            &image,
            &revision,
            &policy,
            &ProductCapabilityBundle::empty(),
        )
        .unwrap_err();

        assert!(matches!(err, WorkbenchError::CapabilityUnavailable { .. }));
    }

    #[test]
    fn infeasible_template_handle_is_skipped_not_fatal() {
        let mut image = image::RgbaImage::from_fn(80, 80, |x, y| {
            let v = 120 + ((x * 3 + y * 5) % 23) as u8;
            image::Rgba([v, v, v, 255])
        });
        for y in 0..8 {
            for x in 0..8 {
                let v = ((x * 17 + y * 31 + x * y * 3) % 255) as u8;
                image.put_pixel(20 + x, 24 + y, image::Rgba([v, v, v, 255]));
            }
        }
        let tpl = image::imageops::crop_imm(&image, 20, 24, 8, 8).to_image();
        let mut store = rollshot_vision::TemplateStore::new();
        store
            .insert(rollshot_vision::TemplateAsset {
                handle: "mark".into(),
                sensitivity: rollshot_vision::TemplateSensitivity::Chrome,
                source: rollshot_vision::TemplateSource::UserRect,
                created_at_ms: 1,
                bounds_in_source_image: None,
                bytes: rollshot_vision::TemplateBytes::new(8, 8, tpl.into_raw()).unwrap(),
            })
            .unwrap();
        store
            .insert(rollshot_vision::TemplateAsset {
                handle: "flat".into(),
                sensitivity: rollshot_vision::TemplateSensitivity::Chrome,
                source: rollshot_vision::TemplateSource::UserRect,
                created_at_ms: 2,
                bounds_in_source_image: None,
                bytes: rollshot_vision::TemplateBytes::new(8, 8, vec![128u8; 8 * 8 * 4]).unwrap(),
            })
            .unwrap();
        let bundle = ProductCapabilityBundle::from_template_store_for_tests(store);
        let source = r#"
function main(input) {
  const matches = rollshot.templateMatch({
    templateHandle: input.capabilityHandles.mark,
    region: { kind: "full" },
    limit: 1
  });
  return { candidates: matches.map((match) => ({
    kind: "addRedaction", bounds: match.bounds, confidence: match.score, label: "mark"
  })) };
}
"#;
        let revision = make_revision_from_source(source);
        let policy = ExecutionPolicy::smart_redaction_default(
            std::time::Duration::from_secs(10),
            100_000_000,
            8_000_000,
        );

        let proposal =
            run_existing_preset_with_capabilities(&image, &revision, &policy, &bundle).unwrap();

        assert_eq!(proposal.candidates.len(), 1);
    }

    #[test]
    fn bundle_loaded_from_disk_drives_existing_preset_match() {
        let tmp = tempfile::tempdir().unwrap();
        let store = rollshot_preset::PresetStore::open(tmp.path().to_path_buf());
        let preset_id = rollshot_preset::PresetId("preset-a".into());
        store
            .create_preset(
                preset_id.clone(),
                "Preset A".into(),
                "test".into(),
                "2026-06-28T00:00:00Z".into(),
            )
            .unwrap();

        let mut image = image::RgbaImage::from_fn(80, 80, |x, y| {
            let v = 120 + ((x * 3 + y * 5) % 23) as u8;
            image::Rgba([v, v, v, 255])
        });
        for y in 0..8 {
            for x in 0..8 {
                let v = ((x * 17 + y * 31 + x * y * 3) % 255) as u8;
                image.put_pixel(20 + x, 24 + y, image::Rgba([v, v, v, 255]));
            }
        }
        let tpl = image::imageops::crop_imm(&image, 20, 24, 8, 8).to_image();
        let mut templates = rollshot_vision::TemplateStore::new();
        templates
            .insert(rollshot_vision::TemplateAsset {
                handle: "mark".into(),
                sensitivity: rollshot_vision::TemplateSensitivity::Chrome,
                source: rollshot_vision::TemplateSource::UserRect,
                created_at_ms: 1,
                bounds_in_source_image: None,
                bytes: rollshot_vision::TemplateBytes::new(8, 8, tpl.into_raw()).unwrap(),
            })
            .unwrap();
        templates
            .save_local(&store.template_store_path(&preset_id).unwrap())
            .unwrap();

        let bundle = ProductCapabilityBundle::load(&store, Some(&preset_id)).unwrap();
        let source = r#"
function main(input) {
  const matches = rollshot.templateMatch({
    templateHandle: input.capabilityHandles.mark,
    region: { kind: "full" },
    limit: 1
  });
  return { candidates: matches.map((match) => ({
    kind: "addRedaction", bounds: match.bounds, confidence: match.score, label: "mark"
  })) };
}
"#;
        let revision = make_revision_from_source(source);
        let policy = ExecutionPolicy::smart_redaction_default(
            std::time::Duration::from_secs(10),
            100_000_000,
            8_000_000,
        );

        let proposal =
            run_existing_preset_with_capabilities(&image, &revision, &policy, &bundle).unwrap();

        assert_eq!(proposal.candidates.len(), 1);
    }
}

#[cfg(test)]
mod prepare_tests {
    use super::*;

    #[test]
    fn prepare_vision_context_rejects_empty_image() {
        let empty = image::RgbaImage::new(0, 0);
        let r = prepare_vision_context(&empty, &ProductCapabilityBundle::empty());
        assert!(matches!(r, Err(WorkbenchError::VisionPrepare { .. })));
    }

    #[test]
    fn prepare_vision_context_succeeds_for_valid_image() {
        let img = image::RgbaImage::from_fn(8, 8, |_, _| image::Rgba([200, 200, 200, 255]));
        let ctx = prepare_vision_context(&img, &ProductCapabilityBundle::empty()).unwrap();
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
        let binding = rollshot_agent::product_task::DocumentContentBinding::new(
            [1u8; 32],
            &rollshot_agent::product_task::AnnotationStateV1 {
                width: 100,
                height: 100,
                state_id: 0,
                annotations: vec![],
            },
            0,
        )
        .unwrap();
        std::sync::Arc::new(
            rollshot_agent::tools::ToolContext::new_with_capability_handles(
                rollshot_agent::domain::SessionId::new(1),
                rollshot_agent::domain::RunId::parse("run-00000000-0000-4000-8000-000000000001")
                    .unwrap(),
                rollshot_edit_proposal::ProposalId::parse(
                    "proposal-00000000-0000-4000-8000-000000000001",
                )
                .unwrap(),
                binding,
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
            ),
        )
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

        let mut expected = vec![
            "replace_source",
            "validate_source",
            "submit_for_review",
            "request_user_input",
            "inspect_context_summary",
            "read_current_source",
            "inspect_image_context",
            "edit_source",
            "inspect_region_features",
        ];
        #[cfg(feature = "ocr")]
        expected.push("inspect_ocr");
        expected.push("dry_run");

        assert_eq!(names, expected);
        #[cfg(not(feature = "ocr"))]
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
                assert!(result_json["capability_handles"]
                    .as_array()
                    .unwrap()
                    .is_empty());
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
                assert!(result_json["capability_handles"]
                    .as_array()
                    .unwrap()
                    .is_empty());
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
        let vision = prepare_vision_context(&image, &ProductCapabilityBundle::empty()).unwrap();
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

    #[tokio::test]
    async fn inspect_image_context_reports_template_handles_available() {
        use rollshot_agent::tools::{InspectImageContextTool, Tool};

        let mut handles = std::collections::BTreeMap::new();
        handles.insert("mark".to_string(), "mark".to_string());
        let cancel = rollshot_agent::runtime::RunCancellation::new();
        let binding = rollshot_agent::product_task::DocumentContentBinding::new(
            [1u8; 32],
            &rollshot_agent::product_task::AnnotationStateV1 {
                width: 100,
                height: 100,
                state_id: 0,
                annotations: vec![],
            },
            0,
        )
        .unwrap();
        let ctx = std::sync::Arc::new(
            rollshot_agent::tools::ToolContext::new_with_capability_handles(
                rollshot_agent::domain::SessionId::new(1),
                rollshot_agent::domain::RunId::parse("run-00000000-0000-4000-8000-000000000001")
                    .unwrap(),
                rollshot_edit_proposal::ProposalId::parse(
                    "proposal-00000000-0000-4000-8000-000000000001",
                )
                .unwrap(),
                binding,
                String::new(),
                rollshot_automation::ValidationLimits::default(),
                rollshot_automation::ExecutionPolicy::smart_redaction_default(
                    std::time::Duration::from_secs(5),
                    4 * 1024 * 1024,
                    1024 * 1024,
                ),
                (64, 64),
                handles,
                &cancel,
            ),
        );
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
                    result_json["capabilities"]["template_match"]["status"].as_str(),
                    Some("available")
                );
                assert_eq!(
                    result_json["capability_handles"][0]["name"].as_str(),
                    Some("mark")
                );
            }
            other => panic!("expected inspection success, got {other:?}"),
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

        let vision = prepare_vision_context(&image, &ProductCapabilityBundle::empty()).unwrap();
        let region_catalog = canonical_region_feature_catalog(480, 160);
        let ocr_catalog = canonical_ocr_catalog(480, 160);
        let inspection = authoring_inspection_context(
            PayloadMode::FullScreenshot,
            &region_catalog,
            &ocr_catalog,
        );
        let cancel = rollshot_agent::runtime::RunCancellation::new();
        let binding = rollshot_agent::product_task::DocumentContentBinding::new(
            [1u8; 32],
            &rollshot_agent::product_task::AnnotationStateV1 {
                width: 100,
                height: 100,
                state_id: 0,
                annotations: vec![],
            },
            0,
        )
        .unwrap();
        let ctx = std::sync::Arc::new(
            rollshot_agent::tools::ToolContext::new_with_capability_handles(
                rollshot_agent::domain::SessionId::new(1),
                rollshot_agent::domain::RunId::parse("run-00000000-0000-4000-8000-000000000001")
                    .unwrap(),
                rollshot_edit_proposal::ProposalId::parse(
                    "proposal-00000000-0000-4000-8000-000000000001",
                )
                .unwrap(),
                binding,
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
            ),
        );
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

    fn textured_template_bytes() -> rollshot_vision::TemplateBytes {
        let rgba = image::RgbaImage::from_fn(8, 8, |x, y| {
            let v = ((x * 17 + y * 31 + x * y * 3) % 255) as u8;
            image::Rgba([v, v, v, 255])
        });
        rollshot_vision::TemplateBytes::new(8, 8, rgba.into_raw()).unwrap()
    }

    #[test]
    fn product_capability_bundle_loads_preset_template_handles() {
        let tmp = tempfile::tempdir().unwrap();
        let store = rollshot_preset::PresetStore::open(tmp.path().to_path_buf());
        let preset_id = rollshot_preset::PresetId("preset-a".into());
        store
            .create_preset(
                preset_id.clone(),
                "Preset A".into(),
                "test".into(),
                "2026-06-28T00:00:00Z".into(),
            )
            .unwrap();
        let mut templates = rollshot_vision::TemplateStore::new();
        templates
            .insert(rollshot_vision::TemplateAsset {
                handle: "toolbar-logo".into(),
                sensitivity: rollshot_vision::TemplateSensitivity::Chrome,
                source: rollshot_vision::TemplateSource::UserRect,
                created_at_ms: 1,
                bounds_in_source_image: None,
                bytes: textured_template_bytes(),
            })
            .unwrap();
        templates
            .save_local(&store.template_store_path(&preset_id).unwrap())
            .unwrap();

        let bundle = ProductCapabilityBundle::load(&store, Some(&preset_id)).unwrap();

        assert_eq!(
            bundle
                .capability_handles
                .get("toolbar-logo")
                .map(String::as_str),
            Some("toolbar-logo")
        );
        assert_eq!(bundle.template_summaries.len(), 1);
        assert!(bundle.availability.template_match.available);
    }

    #[test]
    fn product_capability_bundle_reports_missing_template_store() {
        let tmp = tempfile::tempdir().unwrap();
        let store = rollshot_preset::PresetStore::open(tmp.path().to_path_buf());
        let preset_id = rollshot_preset::PresetId("preset-a".into());
        store
            .create_preset(
                preset_id.clone(),
                "Preset A".into(),
                "test".into(),
                "2026-06-28T00:00:00Z".into(),
            )
            .unwrap();

        let bundle = ProductCapabilityBundle::load(&store, Some(&preset_id)).unwrap();

        assert!(bundle.capability_handles.is_empty());
        assert!(!bundle.availability.template_match.available);
        assert_eq!(
            bundle.availability.template_match.reason.as_deref(),
            Some("no_capability_handles")
        );
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
            id: ProposalId::parse("proposal-00000001-0000-4000-8000-000000000000").unwrap(),
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
        let mut ws = ResultWorkspace::with_config_path(ResultDocument::unsaved(img), None, None);
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

    fn agent_candidate(id: u64, b: ImageRect) -> ProposedCandidate {
        ProposedCandidate {
            id: CandidateId(id),
            edit: ProposedEdit::AddRedaction { bounds: b },
            confidence: 0.9,
            label: "agent".into(),
            rationale: None,
            provenance: Provenance {
                source: ProvenanceSource::Agent {
                    run_id: "run-00000000-0000-4000-8000-000000000007".to_string(),
                },
            },
        }
    }

    fn active_revision_for_reducer_test() -> rollshot_preset::AutomationRevision {
        use rollshot_preset::{
            AutomationRevision, PresetId, RevisionCapabilityMetadata, RevisionId, RevisionOrigin,
            RevisionProvenance, STORE_SCHEMA_VERSION,
        };
        let source = "function main(input) { return { candidates: [] }; }";
        let validated = rollshot_automation::validate_source(
            source,
            &rollshot_automation::ValidationLimits::default(),
        )
        .unwrap();
        AutomationRevision {
            store_schema_version: STORE_SCHEMA_VERSION,
            id: RevisionId("rev-1".into()),
            preset_id: PresetId("workbench-draft".into()),
            parent_id: None,
            created_at: "2026-06-28T00:00:00Z".into(),
            provenance: RevisionProvenance {
                origin: RevisionOrigin::AgentRun,
                note: None,
                source_run_ref: Some("session:7".into()),
            },
            artifact: validated,
            capabilities: RevisionCapabilityMetadata::default(),
        }
    }

    fn seed_active_revision_pending_proposal_and_rejection(ws: &mut ResultWorkspace) {
        let wb = wb_mut(ws);
        wb.active_revision = Some(active_revision_for_reducer_test());
        let p = proposal(vec![agent_candidate(1, rect(10.0, 10.0, 50.0, 50.0))]);
        wb.pending_proposal = Some(p);
        wb.review = super::super::CandidateReview::from_candidates(&[CandidateId(1)]);
        wb.review.mark_rejected(CandidateId(1));
    }

    #[test]
    fn run_terminal_ready_for_review_populates_proposal_review_draft() {
        use rollshot_agent::domain::SessionId;
        use rollshot_agent::driver::{DraftAutomation, DryRunEvidence, ReadyForReview};
        use rollshot_agent::runtime::UsageSnapshot;

        let mut ws = ws_with_workbench();
        let (tid, rid) = test_run_ids();
        wb_mut(&mut ws).run_state = super::super::RunState::Running {
            cancellation: rollshot_agent::runtime::RunCancellation::new(),
            parent_revision_id: None,
            revision_note: None,
            task_id: tid.clone(),
            run_id: rid.clone(),
        };
        wb_mut(&mut ws).selected_candidate = Some(CandidateId(99));
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
            Message::Workbench(WorkbenchMessage::RunTerminal {
                task_id: tid,
                run_id: rid,
                terminal: RunTerminalState::ReadyForReview(Box::new(ready)),
            }),
        );
        let state = wb(&ws);
        assert!(state.pending_proposal.is_some(), "proposal populated");
        assert_eq!(state.pending_proposal.as_ref().unwrap().candidates.len(), 2);
        assert_eq!(state.review.per_candidate.len(), 2);
        assert!(state.pending_draft.is_some(), "draft populated");
        assert_eq!(state.pending_draft.as_ref().unwrap().assistant_text, "done");
        assert!(
            state.selected_candidate.is_none(),
            "fresh proposal clears stale selection"
        );
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
    fn discard_candidates_clears_pending_review_state_without_applying() {
        let mut ws = ws_with_workbench();
        let p = proposal(vec![candidate(1, rect(10.0, 10.0, 50.0, 50.0))]);
        let review = super::super::CandidateReview::from_candidates(&[CandidateId(1)]);
        wb_mut(&mut ws).pending_proposal = Some(p);
        wb_mut(&mut ws).review = review;
        wb_mut(&mut ws).selected_candidate = Some(CandidateId(1));
        wb_mut(&mut ws).corrections_non_empty = true;

        let _ = update(
            &mut ws,
            Message::Workbench(WorkbenchMessage::DiscardCandidates),
        );

        let state = wb(&ws);
        assert!(state.pending_proposal.is_none(), "proposal cleared");
        assert!(state.review.is_empty(), "review cleared");
        assert!(state.selected_candidate.is_none(), "selection cleared");
        assert!(
            !state.corrections_non_empty,
            "revision evidence cache cleared"
        );
        assert_eq!(
            ws.document.image.annotations().len(),
            0,
            "discard does not apply annotations"
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
        wb_mut(&mut ws).pending_run = Some(test_pending_run_params());

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
        let (tid, rid) = test_run_ids();
        wb_mut(&mut ws).run_state = super::super::RunState::Running {
            cancellation: rollshot_agent::runtime::RunCancellation::new(),
            parent_revision_id: None,
            revision_note: None,
            task_id: tid.clone(),
            run_id: rid.clone(),
        };
        let _ = update(
            &mut ws,
            Message::Workbench(WorkbenchMessage::RunEvent {
                task_id: tid,
                run_id: rid,
                event: RunEvent::TextChunk {
                    text: "hello".into(),
                },
            }),
        );
        let state = wb(&ws);
        assert_eq!(state.live_activity.len(), 1);
    }

    #[test]
    fn text_chunks_accumulate_into_one_entry() {
        use rollshot_agent::runtime::RunEvent;

        let mut ws = ws_with_workbench();
        let (tid, rid) = test_run_ids();
        wb_mut(&mut ws).run_state = super::super::RunState::Running {
            cancellation: rollshot_agent::runtime::RunCancellation::new(),
            parent_revision_id: None,
            revision_note: None,
            task_id: tid.clone(),
            run_id: rid.clone(),
        };
        let _ = update(
            &mut ws,
            Message::Workbench(WorkbenchMessage::RunEvent {
                task_id: tid.clone(),
                run_id: rid.clone(),
                event: RunEvent::TextChunk {
                    text: "hello ".into(),
                },
            }),
        );
        let _ = update(
            &mut ws,
            Message::Workbench(WorkbenchMessage::RunEvent {
                task_id: tid,
                run_id: rid,
                event: RunEvent::TextChunk {
                    text: "world".into(),
                },
            }),
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
        let (tid, rid) = test_run_ids();
        wb_mut(&mut ws).run_state = super::super::RunState::Running {
            cancellation: rollshot_agent::runtime::RunCancellation::new(),
            parent_revision_id: None,
            revision_note: None,
            task_id: tid.clone(),
            run_id: rid.clone(),
        };
        // Streamed chunks (may have gaps from dropped try_send).
        let _ = update(
            &mut ws,
            Message::Workbench(WorkbenchMessage::RunEvent {
                task_id: tid.clone(),
                run_id: rid.clone(),
                event: RunEvent::TextChunk {
                    text: "hel".into(),
                },
            }),
        );
        let _ = update(
            &mut ws,
            Message::Workbench(WorkbenchMessage::RunEvent {
                task_id: tid.clone(),
                run_id: rid.clone(),
                event: RunEvent::TextChunk {
                    text: "lo".into(),
                },
            }),
        );
        // Terminal with authoritative full text.
        let ready = ready_for_review_with_text("hello world");
        let _ = update(
            &mut ws,
            Message::Workbench(WorkbenchMessage::RunTerminal {
                task_id: tid,
                run_id: rid,
                terminal: RunTerminalState::ReadyForReview(Box::new(ready)),
            }),
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
                id: rollshot_edit_proposal::ProposalId::parse(
                    "proposal-00000001-0000-4000-8000-000000000000",
                )
                .unwrap(),
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

    fn test_pending_run_params() -> super::super::PendingRunParams {
        super::super::PendingRunParams {
            user_message: "test".into(),
            image_dims: (100, 100),
            active_revision_source: None,
            mode: super::super::RunKind::Author,
            parent_revision_id: None,
            revision_note: None,
            preset_id: rollshot_preset::PresetId("workbench-draft".into()),
            preset_store_root: std::path::PathBuf::from("/tmp/rollshot-test-presets"),
            task_id: rollshot_agent::product_task::ProductTaskId::parse(
                "task-00000000-0000-4000-8000-000000000001",
            )
            .unwrap(),
            run_id: rollshot_agent::domain::RunId::parse(
                "run-00000000-0000-4000-8000-000000000001",
            )
            .unwrap(),
            proposal_id: rollshot_edit_proposal::ProposalId::parse(
                "proposal-00000000-0000-4000-8000-000000000001",
            )
            .unwrap(),
            artifact_id: rollshot_agent::product_task::ArtifactId::parse(
                "artifact-00000000-0000-4000-8000-000000000001",
            )
            .unwrap(),
            content_binding: rollshot_agent::product_task::DocumentContentBinding::new(
                [1u8; 32],
                &rollshot_agent::product_task::AnnotationStateV1 {
                    width: 100,
                    height: 100,
                    state_id: 0,
                    annotations: vec![],
                },
                0,
            )
            .unwrap(),
        }
    }

    fn test_run_ids() -> (rollshot_agent::product_task::ProductTaskId, rollshot_agent::domain::RunId) {
        (
            rollshot_agent::product_task::ProductTaskId::parse("task-00000000-0000-4000-8000-000000000001").unwrap(),
            rollshot_agent::domain::RunId::parse("run-00000000-0000-4000-8000-000000000001").unwrap(),
        )
    }

    #[test]
    fn cancel_run_calls_cancellation() {
        use rollshot_agent::runtime::RunCancellation;

        let mut ws = ws_with_workbench();
        let cancel = RunCancellation::new();
        let (tid, rid) = test_run_ids();
        wb_mut(&mut ws).run_state = super::super::RunState::Running {
            cancellation: cancel.clone(),
            parent_revision_id: None,
            revision_note: None,
            task_id: tid,
            run_id: rid,
        };

        let _ = update(&mut ws, Message::Workbench(WorkbenchMessage::CancelRun));
        assert!(cancel.is_cancelled());
    }

    #[test]
    fn run_failed_sets_error_and_terminal() {
        let mut ws = ws_with_workbench();
        let (tid, rid) = test_run_ids();
        wb_mut(&mut ws).run_state = super::super::RunState::Running {
            cancellation: rollshot_agent::runtime::RunCancellation::new(),
            parent_revision_id: None,
            revision_note: None,
            task_id: tid.clone(),
            run_id: rid.clone(),
        };

        let _ = update(
            &mut ws,
            Message::Workbench(WorkbenchMessage::RunFailed {
                task_id: tid,
                run_id: rid,
                error: super::WorkbenchError::VisionPrepare {
                    message: "region_too_large".into(),
                },
            }),
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
        let (tid, rid) = test_run_ids();
        wb_mut(&mut ws).run_state = super::super::RunState::Running {
            cancellation: rollshot_agent::runtime::RunCancellation::new(),
            parent_revision_id: None,
            revision_note: None,
            task_id: tid,
            run_id: rid,
        };
        wb_mut(&mut ws).disclosure_pending = true;
        wb_mut(&mut ws).pending_run = Some(test_pending_run_params());

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

    #[test]
    fn run_terminal_carries_lineage_into_pending_draft() {
        let mut ws = ws_with_workbench();
        let (tid, rid) = test_run_ids();
        wb_mut(&mut ws).run_state = super::super::RunState::Running {
            cancellation: rollshot_agent::runtime::RunCancellation::new(),
            parent_revision_id: Some(rollshot_preset::RevisionId("rev-parent".into())),
            revision_note: Some(
                "improved from rev-parent; 1 rejected, 0 resized, 0 manually added".into(),
            ),
            task_id: tid.clone(),
            run_id: rid.clone(),
        };
        let ready = ready_for_review_with_text("done");
        let _ = update(
            &mut ws,
            Message::Workbench(WorkbenchMessage::RunTerminal {
                task_id: tid,
                run_id: rid,
                terminal: RunTerminalState::ReadyForReview(Box::new(ready)),
            }),
        );
        let draft = wb(&ws).pending_draft.as_ref().expect("draft populated");
        assert_eq!(draft.parent_revision_id.as_ref().unwrap().0, "rev-parent");
        assert!(draft.revision_note.as_ref().unwrap().contains("1 rejected"));
    }

    #[test]
    fn ask_agent_to_revise_queues_improve_run_with_correction_evidence() {
        let mut ws = ws_with_workbench();
        seed_active_revision_pending_proposal_and_rejection(&mut ws);

        let task = update(
            &mut ws,
            Message::Workbench(WorkbenchMessage::AskAgentToRevise),
        );
        drop(task);

        let state = wb(&ws);
        let params = state.pending_run.as_ref().expect("pending improve run");
        assert_eq!(params.mode, super::super::RunKind::Improve);
        assert!(params.user_message.contains("Rejected false positives"));
        assert!(params
            .active_revision_source
            .as_ref()
            .unwrap()
            .contains("function main"));
        assert_eq!(params.parent_revision_id.as_ref().unwrap().0, "rev-1");
        assert!(params
            .revision_note
            .as_ref()
            .unwrap()
            .contains("1 rejected"));
        assert!(state.disclosure_pending);
    }

    #[test]
    fn im_start_queues_improve_run_with_correction_evidence() {
        let mut ws = ws_with_workbench();
        seed_active_revision_pending_proposal_and_rejection(&mut ws);

        let task = update(&mut ws, Message::Workbench(WorkbenchMessage::ImStart));
        drop(task);

        let state = wb(&ws);
        let params = state
            .pending_run
            .as_ref()
            .expect("pending improve run from banner action");
        assert_eq!(params.mode, super::super::RunKind::Improve);
        assert!(params.user_message.contains("Rejected false positives"));
        assert_eq!(params.parent_revision_id.as_ref().unwrap().0, "rev-1");
        assert!(state.disclosure_pending);
    }

    #[test]
    fn manual_candidate_uses_non_colliding_id_for_missed_target_evidence() {
        let mut ws = ws_with_workbench();
        {
            let wb = wb_mut(&mut ws);
            wb.pending_proposal = Some(proposal(vec![agent_candidate(
                1,
                rect(10.0, 10.0, 50.0, 50.0),
            )]));
            wb.review = super::super::CandidateReview::from_candidates(&[CandidateId(1)]);
        }

        let manual_bounds = rect(80.0, 20.0, 12.0, 8.0);
        let _ = update(
            &mut ws,
            Message::Workbench(WorkbenchMessage::AddManualCandidate {
                bounds: manual_bounds,
            }),
        );

        let state = wb(&ws);
        let proposal = state.pending_proposal.as_ref().unwrap();
        assert_eq!(proposal.candidates[0].id, CandidateId(1));
        assert_eq!(proposal.candidates[1].id, CandidateId(2));

        let evidence = super::super::review::assemble_correction_evidence(proposal, &state.review);
        assert!(evidence.resized.is_empty());
        assert_eq!(evidence.manual_added.len(), 1);
        assert_eq!(evidence.manual_added[0].id, CandidateId(2));
        assert_eq!(evidence.manual_added[0].bounds, manual_bounds);
    }

    #[test]
    fn ask_agent_to_revise_is_noop_without_corrections() {
        let mut ws = ws_with_workbench();
        // Active revision + proposal present, but the review has no rejections,
        // resizes, or manual additions → empty evidence → silent no-op.
        // Scope the mutable borrow in a block so the local does not shadow the
        // `wb(&ws)` accessor used below.
        {
            let wb = wb_mut(&mut ws);
            wb.active_revision = Some(active_revision_for_reducer_test());
            wb.pending_proposal = Some(proposal(vec![agent_candidate(
                1,
                rect(10.0, 10.0, 50.0, 50.0),
            )]));
            wb.review = super::super::CandidateReview::from_candidates(&[CandidateId(1)]);
        }

        let _ = update(
            &mut ws,
            Message::Workbench(WorkbenchMessage::AskAgentToRevise),
        );
        let state = wb(&ws);
        assert!(
            state.pending_run.is_none(),
            "no run queued without corrections"
        );
        assert!(!state.disclosure_pending, "disclosure not opened");
    }

    #[test]
    fn stale_run_messages_are_ignored() {
        use rollshot_agent::runtime::RunEvent;

        let mut ws = ws_with_workbench();

        // Set up a "current" run with specific task_id/run_id.
        let current_task = rollshot_agent::product_task::ProductTaskId::parse(
            "task-00000000-0000-4000-8000-000000000099",
        )
        .unwrap();
        let current_run = rollshot_agent::domain::RunId::parse(
            "run-00000000-0000-4000-8000-000000000099",
        )
        .unwrap();
        wb_mut(&mut ws).run_state = super::super::RunState::Running {
            cancellation: rollshot_agent::runtime::RunCancellation::new(),
            parent_revision_id: None,
            revision_note: None,
            task_id: current_task.clone(),
            run_id: current_run.clone(),
        };

        // A stale message from a different run.
        let stale_task = rollshot_agent::product_task::ProductTaskId::parse(
            "task-00000000-0000-4000-8000-000000000001",
        )
        .unwrap();
        let stale_run = rollshot_agent::domain::RunId::parse(
            "run-00000000-0000-4000-8000-000000000001",
        )
        .unwrap();

        // Stale RunEvent should be ignored.
        let _ = update(
            &mut ws,
            Message::Workbench(WorkbenchMessage::RunEvent {
                task_id: stale_task.clone(),
                run_id: stale_run.clone(),
                event: RunEvent::TextChunk {
                    text: "stale".into(),
                },
            }),
        );
        assert!(
            wb(&ws).live_activity.is_empty(),
            "stale RunEvent must not produce activity"
        );

        // Stale RunFailed should be ignored.
        let _ = update(
            &mut ws,
            Message::Workbench(WorkbenchMessage::RunFailed {
                task_id: stale_task.clone(),
                run_id: stale_run.clone(),
                error: super::WorkbenchError::RuntimeFailure,
            }),
        );
        assert!(
            wb(&ws).error.is_none(),
            "stale RunFailed must not set error"
        );
        assert!(
            wb(&ws).run_state.is_running(),
            "stale RunFailed must not change run state"
        );

        // Stale RunTerminal should be ignored.
        let ready = ready_for_review_with_text("stale terminal");
        let _ = update(
            &mut ws,
            Message::Workbench(WorkbenchMessage::RunTerminal {
                task_id: stale_task,
                run_id: stale_run,
                terminal: rollshot_agent::driver::RunTerminalState::ReadyForReview(
                    Box::new(ready),
                ),
            }),
        );
        assert!(
            wb(&ws).run_state.is_running(),
            "stale RunTerminal must not change run state"
        );
        assert!(
            wb(&ws).pending_proposal.is_none(),
            "stale RunTerminal must not populate proposal"
        );
    }

    #[test]
    fn correlated_run_messages_are_accepted() {
        use rollshot_agent::runtime::RunEvent;

        let mut ws = ws_with_workbench();
        let (tid, rid) = test_run_ids();
        wb_mut(&mut ws).run_state = super::super::RunState::Running {
            cancellation: rollshot_agent::runtime::RunCancellation::new(),
            parent_revision_id: None,
            revision_note: None,
            task_id: tid.clone(),
            run_id: rid.clone(),
        };

        // Matching RunEvent should be accepted.
        let _ = update(
            &mut ws,
            Message::Workbench(WorkbenchMessage::RunEvent {
                task_id: tid.clone(),
                run_id: rid.clone(),
                event: RunEvent::TextChunk {
                    text: "hello".into(),
                },
            }),
        );
        assert_eq!(
            wb(&ws).live_activity.len(),
            1,
            "correlated RunEvent must produce activity"
        );
    }

    // -- Persistence-ordering tests (Finding 2) ----------------------------

    #[test]
    fn running_is_persisted_before_setup() {
        use rollshot_agent::product_task::{
            ProductTaskSnapshot, SourceBinding, TaskAttempt, TaskAttemptId, TaskKind,
        };
        use super::super::task_store::TaskStore;

        let tmp = tempfile::tempdir().unwrap();
        let store = TaskStore::open(tmp.path()).unwrap();
        let task_id = rollshot_agent::product_task::ProductTaskId::parse(
            "task-00000000-0000-4000-8000-000000000001",
        )
        .unwrap();
        let run_id = rollshot_agent::domain::RunId::parse(
            "run-00000000-0000-4000-8000-000000000001",
        )
        .unwrap();

        // Simulate what the stream does: create a running snapshot.
        let now = chrono::Utc::now().timestamp_millis();
        let source_binding = SourceBinding::new([0u8; 32], [0u8; 32], 0, "test".into(), None);
        let snapshot = ProductTaskSnapshot::new(
            task_id.clone(),
            TaskKind::SmartRedactionAuthor,
            source_binding,
            now,
        )
        .unwrap();
        let attempt = TaskAttempt::new(TaskAttemptId::new(1), run_id.clone(), now);
        let running = snapshot.start_attempt(attempt, now).unwrap();

        // Persist running snapshot.
        store.create(&running).unwrap();

        // Verify: running snapshot exists in store before any setup.
        let loaded = store.load(&task_id).unwrap();
        assert_eq!(loaded.status(), rollshot_agent::product_task::TaskStatus::Running);
        assert_eq!(loaded.task_id(), &task_id);
    }

    #[test]
    fn ready_artifact_precedes_correlated_terminal() {
        use rollshot_agent::product_task::{
            ArtifactId, ArtifactKind, ArtifactRevision, ProductArtifactMetadata,
            ProductTaskSnapshot, PayloadConfigV1, PayloadDryRunV1, PayloadProposalV1,
            PayloadSourceV1, SmartRedactionReviewPayload, SourceBinding,
            TaskAttempt, TaskAttemptId, TaskKind, TaskStatus,
        };
        use super::super::task_store::TaskStore;

        let tmp = tempfile::tempdir().unwrap();
        let store = TaskStore::open(tmp.path()).unwrap();
        let task_id = rollshot_agent::product_task::ProductTaskId::parse(
            "task-00000000-0000-4000-8000-000000000002",
        )
        .unwrap();
        let run_id = rollshot_agent::domain::RunId::parse(
            "run-00000000-0000-4000-8000-000000000002",
        )
        .unwrap();

        // Create and persist running snapshot.
        let now = chrono::Utc::now().timestamp_millis();
        let source_binding = SourceBinding::new([0u8; 32], [0u8; 32], 0, "test".into(), None);
        let snapshot = ProductTaskSnapshot::new(
            task_id.clone(),
            TaskKind::SmartRedactionAuthor,
            source_binding,
            now,
        )
        .unwrap();
        let attempt = TaskAttempt::new(TaskAttemptId::new(1), run_id.clone(), now);
        let running = snapshot.start_attempt(attempt, now).unwrap();
        store.create(&running).unwrap();

        // Simulate ReadyForReview: persist artifact via CAS.
        let now2 = now + 1;
        let metadata = ProductArtifactMetadata::new(
            ArtifactId::parse("artifact-00000000-0000-4000-8000-000000000002").unwrap(),
            ArtifactRevision::new(1),
            ArtifactKind::SmartRedaction,
            1,
            String::new(),
            SourceBinding::new([1u8; 32], [0u8; 32], 0, "test".into(), None),
            task_id.clone(),
            running.attempts().last().unwrap().attempt_id(),
            run_id.clone(),
            "proposal-test".into(),
            String::new(),
            String::new(),
            String::new(),
            0,
            0.0,
            now2,
        );
        let payload = SmartRedactionReviewPayload {
            source: PayloadSourceV1 {
                kind: "agent_run".into(),
                validation_summary: "0 nodes".into(),
            },
            proposal: PayloadProposalV1 {
                proposal_id: "proposal-test".into(),
                candidate_count: 0,
            },
            dry_run: PayloadDryRunV1 {
                candidate_count: 0,
                affected_area: 0.0,
            },
            config: PayloadConfigV1 {
                provider: String::new(),
                model: String::new(),
                payload_mode: rollshot_agent::product_task::PayloadMode::Author,
                run_kind: "smart_redaction".into(),
                budget_dimensions: std::collections::BTreeMap::new(),
            },
        };
        let ready = running.record_ready_for_review(metadata, payload, now2).unwrap();
        store.compare_and_swap(&running, &ready).unwrap();

        // Verify: artifact is in store (ReadyForReview persisted before terminal).
        let loaded = store.load(&task_id).unwrap();
        assert_eq!(loaded.status(), TaskStatus::ReadyForReview);
        assert!(loaded.artifact_metadata().is_some(), "artifact persisted");
        assert!(loaded.pending_artifact_payload().is_some(), "payload persisted");
    }

    #[test]
    fn store_precommit_failure_delivers_no_proposal() {
        use rollshot_agent::product_task::{
            ProductTaskSnapshot, SourceBinding, TaskAttempt, TaskAttemptId, TaskKind,
        };
        use super::super::task_store::{Failpoint, TaskStore};

        let tmp = tempfile::tempdir().unwrap();
        let store = TaskStore::open_with_failpoint(tmp.path(), Failpoint::Rename).unwrap();
        let task_id = rollshot_agent::product_task::ProductTaskId::parse(
            "task-00000000-0000-4000-8000-000000000003",
        )
        .unwrap();
        let run_id = rollshot_agent::domain::RunId::parse(
            "run-00000000-0000-4000-8000-000000000003",
        )
        .unwrap();

        // Create running snapshot (without failpoint).
        let now = chrono::Utc::now().timestamp_millis();
        let source_binding = SourceBinding::new([0u8; 32], [0u8; 32], 0, "test".into(), None);
        let snapshot = ProductTaskSnapshot::new(
            task_id.clone(),
            TaskKind::SmartRedactionAuthor,
            source_binding,
            now,
        )
        .unwrap();
        let attempt = TaskAttempt::new(TaskAttemptId::new(1), run_id.clone(), now);
        let running = snapshot.start_attempt(attempt, now).unwrap();
        store.create_without_failpoint(&running).unwrap();

        // Attempt CAS with failpoint → should fail.
        let now2 = now + 1;
        let terminal_snap = running
            .record_terminal(rollshot_agent::product_task::TaskTerminal::RuntimeFailure, now2)
            .unwrap();
        let result = store.compare_and_swap(&running, &terminal_snap);
        assert!(result.is_err(), "CAS must fail with rename failpoint");

        // Verify: snapshot is still Running (terminal not persisted).
        let loaded = store.load(&task_id).unwrap();
        assert_eq!(
            loaded.status(),
            rollshot_agent::product_task::TaskStatus::Running,
            "must still be running after failed CAS"
        );
    }

    #[test]
    fn setup_failure_persists_terminal() {
        use rollshot_agent::product_task::{
            ProductTaskSnapshot, SourceBinding, TaskAttempt, TaskAttemptId,
            TaskKind, TaskStatus, TaskTerminal,
        };
        use super::super::task_store::TaskStore;

        let tmp = tempfile::tempdir().unwrap();
        let store = TaskStore::open(tmp.path()).unwrap();
        let task_id = rollshot_agent::product_task::ProductTaskId::parse(
            "task-00000000-0000-4000-8000-000000000004",
        )
        .unwrap();
        let run_id = rollshot_agent::domain::RunId::parse(
            "run-00000000-0000-4000-8000-000000000004",
        )
        .unwrap();

        // Create and persist running snapshot.
        let now = chrono::Utc::now().timestamp_millis();
        let source_binding = SourceBinding::new([0u8; 32], [0u8; 32], 0, "test".into(), None);
        let snapshot = ProductTaskSnapshot::new(
            task_id.clone(),
            TaskKind::SmartRedactionAuthor,
            source_binding,
            now,
        )
        .unwrap();
        let attempt = TaskAttempt::new(TaskAttemptId::new(1), run_id.clone(), now);
        let running = snapshot.start_attempt(attempt, now).unwrap();
        store.create(&running).unwrap();

        // Simulate setup failure: persist terminal via CAS.
        let now2 = now + 1;
        let terminal_snap = running
            .record_terminal(TaskTerminal::RuntimeFailure, now2)
            .unwrap();
        store.compare_and_swap(&running, &terminal_snap).unwrap();

        // Verify: terminal persisted before RunFailed would be yielded.
        let loaded = store.load(&task_id).unwrap();
        assert_eq!(
            loaded.status(),
            TaskStatus::Failed { terminal: TaskTerminal::RuntimeFailure },
        );
    }

    // -- Enhanced stale message guard (Finding 3) ---------------------------

    #[test]
    fn stale_messages_rejected_when_terminal() {
        use rollshot_agent::runtime::RunEvent;

        let mut ws = ws_with_workbench();
        // Transition to Terminal state.
        wb_mut(&mut ws).run_state = super::super::RunState::Terminal(
            rollshot_agent::driver::RunTerminalState::RuntimeFailure,
        );

        let task_id = rollshot_agent::product_task::ProductTaskId::parse(
            "task-00000000-0000-4000-8000-000000000099",
        )
        .unwrap();
        let run_id = rollshot_agent::domain::RunId::parse(
            "run-00000000-0000-4000-8000-000000000099",
        )
        .unwrap();

        // RunEvent must be rejected when Terminal.
        let _ = update(
            &mut ws,
            Message::Workbench(WorkbenchMessage::RunEvent {
                task_id: task_id.clone(),
                run_id: run_id.clone(),
                event: RunEvent::TextChunk {
                    text: "late".into(),
                },
            }),
        );
        assert!(
            wb(&ws).live_activity.is_empty(),
            "RunEvent must be rejected when Terminal"
        );

        // RunFailed must be rejected when Terminal.
        let _ = update(
            &mut ws,
            Message::Workbench(WorkbenchMessage::RunFailed {
                task_id: task_id.clone(),
                run_id: run_id.clone(),
                error: super::WorkbenchError::RuntimeFailure,
            }),
        );
        assert!(
            wb(&ws).error.is_none(),
            "RunFailed must be rejected when Terminal"
        );
    }

    #[test]
    fn stale_messages_rejected_when_idle() {
        use rollshot_agent::runtime::RunEvent;

        let mut ws = ws_with_workbench();
        // Idle state (default).
        assert!(wb(&ws).run_state.is_idle());

        let task_id = rollshot_agent::product_task::ProductTaskId::parse(
            "task-00000000-0000-4000-8000-000000000099",
        )
        .unwrap();
        let run_id = rollshot_agent::domain::RunId::parse(
            "run-00000000-0000-4000-8000-000000000099",
        )
        .unwrap();

        // All run messages must be rejected when Idle.
        let _ = update(
            &mut ws,
            Message::Workbench(WorkbenchMessage::RunEvent {
                task_id: task_id.clone(),
                run_id: run_id.clone(),
                event: RunEvent::TextChunk {
                    text: "orphan".into(),
                },
            }),
        );
        assert!(
            wb(&ws).live_activity.is_empty(),
            "RunEvent must be rejected when Idle"
        );

        let _ = update(
            &mut ws,
            Message::Workbench(WorkbenchMessage::RunFailed {
                task_id: task_id.clone(),
                run_id: run_id.clone(),
                error: super::WorkbenchError::RuntimeFailure,
            }),
        );
        assert!(
            wb(&ws).error.is_none(),
            "RunFailed must be rejected when Idle"
        );

        let ready = ready_for_review_with_text("orphan terminal");
        let _ = update(
            &mut ws,
            Message::Workbench(WorkbenchMessage::RunTerminal {
                task_id,
                run_id,
                terminal: rollshot_agent::driver::RunTerminalState::ReadyForReview(
                    Box::new(ready),
                ),
            }),
        );
        assert!(
            wb(&ws).pending_proposal.is_none(),
            "RunTerminal must be rejected when Idle"
        );
    }
}
