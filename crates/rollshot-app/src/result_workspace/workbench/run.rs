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
    let _index = VisualIndex::build(image.clone()).map_err(|e| WorkbenchError::VisionPrepare {
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

pub fn prepare_vision_context(
    image: &image::RgbaImage,
) -> Result<super::VisionContext, WorkbenchError> {
    let index = VisualIndex::build(image.clone()).map_err(|e| WorkbenchError::VisionPrepare {
        message: format!("VisualIndex: {e}"),
    })?;
    let host = rollshot_vision::RealAutomationHost::new();
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
        tools::{
            DryRunTool, GetContextSummaryTool, ReplaceSourceTool, RequestUserInputTool,
            SubmitForReviewTool, ToolContext, ToolRegistry, ToolRegistryLimits, ValidateSourceTool,
        },
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
        let tool_ctx = Arc::new(ToolContext::new(
            session_id,
            active_source,
            validation_limits,
            policy,
            image_dims,
            &cancellation_for_task,
        ));

        let mut registry = ToolRegistry::new(ToolRegistryLimits::permissive());
        let reg = |r: &mut ToolRegistry, t: Arc<dyn rollshot_agent::tools::Tool>| -> Result<(), WorkbenchError> {
            r.register(t).map_err(|_| WorkbenchError::RuntimeFailure)
        };
        reg(&mut registry, Arc::new(ReplaceSourceTool::new(tool_ctx.clone()))).unwrap();
        reg(&mut registry, Arc::new(ValidateSourceTool::new(tool_ctx.clone()))).unwrap();
        reg(&mut registry, Arc::new(SubmitForReviewTool::new(tool_ctx.clone()))).unwrap();
        reg(&mut registry, Arc::new(RequestUserInputTool::new(tool_ctx.clone()))).unwrap();
        reg(&mut registry, Arc::new(GetContextSummaryTool::new(tool_ctx.clone()))).unwrap();
        reg(&mut registry, Arc::new(DryRunTool::new(
            tool_ctx.clone(),
            Arc::new(vision.executor),
            vision.host.clone() as Arc<StdMutex<dyn rollshot_automation::AutomationHost>>,
        ))).unwrap();

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
        assert_eq!(assistant_text, Some("hello world"), "reconciled to authoritative text");
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
        wb_mut(&mut ws).run_state =
            super::super::RunState::Running {
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
}
