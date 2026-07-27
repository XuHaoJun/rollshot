use std::sync::Arc;
use std::sync::Mutex as StdMutex;

use rollshot_agent::domain::{AttachmentDescriptor, AuthorizedModelInput, MediaType, RunId, SessionId};
use rollshot_agent::driver::{AgentConfig, AgentRunner, RunTerminalState};
use rollshot_agent::runtime::{RunBudget, RunCancellation, RunEvent, RunEventSink};
use rollshot_agent::tools::ToolContext;
use rollshot_agent::AnthropicAdapter;
use rollshot_edit_proposal::ProposedEdit;
use rollshot_image_document::ImageRect;
use wiremock::{Mock, MockServer};

use super::cassette::{CassetteFile, CassetteResponder};
use super::fixture::FixtureMeta;
use crate::result_workspace::workbench::run::{
    authoring_inspection_context, build_authoring_tool_registry, canonical_ocr_catalog,
    canonical_region_feature_catalog, prepare_vision_context, product_capability_handles,
    ProductCapabilityBundle,
};
use crate::result_workspace::workbench::PayloadMode;

struct NullSink;
impl RunEventSink for NullSink {
    fn emit(&self, _event: RunEvent) {}
}

pub(crate) async fn replay_full_loop(
    image: &image::RgbaImage,
    meta: &FixtureMeta,
    cassette: &CassetteFile,
) -> Result<Vec<ImageRect>, String> {
    let (w, h) = image.dimensions();

    // 1. Serve the cassette in recorded order.
    let server = MockServer::start().await;
    Mock::given(wiremock::matchers::any())
        .respond_with(CassetteResponder::new(cassette.interactions.clone()))
        .mount(&server)
        .await;
    let adapter =
        AnthropicAdapter::new("test-key", &server.uri()).map_err(|e| format!("adapter: {e:?}"))?;

    // 2. Build the genuine product authoring wiring.
    let vision = prepare_vision_context(image, &ProductCapabilityBundle::empty())
        .map_err(|e| format!("prepare: {e:?}"))?;
    let cancellation = RunCancellation::new();
    let tool_ctx = Arc::new(ToolContext::new_with_capability_handles(
        SessionId::new(1),
        RunId::parse("run-00000000-0000-4000-8000-000000000001").unwrap(),
        String::new(),
        rollshot_automation::ValidationLimits::default(),
        rollshot_automation::ExecutionPolicy::smart_redaction_default(
            std::time::Duration::from_secs(5),
            16 * 1024 * 1024,
            256 * 1024,
        ),
        (w, h),
        product_capability_handles(),
        &cancellation,
    ));
    let inspection = authoring_inspection_context(
        PayloadMode::FullScreenshot,
        &canonical_region_feature_catalog(w, h),
        &canonical_ocr_catalog(w, h),
    );
    let host = vision.host.clone() as Arc<StdMutex<dyn rollshot_automation::AutomationHost>>;
    let executor: Arc<dyn rollshot_automation::AutomationExecutor> =
        Arc::new(rollshot_automation_rquickjs::QuickJsExecutor);
    let registry = build_authoring_tool_registry(tool_ctx.clone(), executor, host, inspection)
        .map_err(|e| format!("registry: {e:?}"))?;

    // 3. Build the model input with the screenshot attachment.
    let mut png = Vec::new();
    image::DynamicImage::ImageRgba8(image.clone())
        .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
        .map_err(|e| format!("png: {e}"))?;
    let descriptor = AttachmentDescriptor {
        media_type: MediaType::Png,
        width: w,
        height: h,
        byte_count: png.len() as u64,
    };
    let input = AuthorizedModelInput::new(
        meta.provider.clone(),
        meta.model.clone(),
        format!("Redact the {} in this screenshot.", meta.intent),
        vec![descriptor],
        vec![png],
    )
    .map_err(|e| format!("input: {e:?}"))?;

    // 4. Run the full loop against the replayed cassette.
    let runner = AgentRunner::new(AgentConfig::default());
    let mut session = rollshot_agent::domain::AgentSession::new(SessionId::new(1), RunId::parse("run-00000000-0000-4000-8000-000000000001").unwrap());
    let terminal = runner
        .run_with_provider(
            input,
            &mut session,
            &registry,
            RunBudget::unlimited(),
            &cancellation,
            &NullSink,
            &tool_ctx,
            &adapter,
        )
        .await;

    let proposal = match terminal {
        RunTerminalState::ReadyForReview(r) => r.proposal,
        other => return Err(format!("non-terminal-ready: {other:?}")),
    };
    Ok(proposal
        .candidates
        .into_iter()
        .filter_map(|c| match c.edit {
            ProposedEdit::AddRedaction { bounds } => Some(bounds),
            _ => None,
        })
        .collect())
}
