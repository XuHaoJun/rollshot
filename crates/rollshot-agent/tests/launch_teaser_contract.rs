//! End-to-end bounded launch-teaser skill contract tests.
//!
//! Exercises the full flow: repository grant → auxiliary read → terminal
//! submission, verifying privacy, receipts, and authority.

use std::collections::BTreeSet;
use std::sync::Arc;

use rollshot_agent::authority::{
    AuthorityBinding, AuthoritySnapshot, AuthoritySubject, DisclosureCeiling, RunOperation,
};
use rollshot_agent::domain::RunId;
use rollshot_agent::driver::{
    compose_launch_teaser_prompt, AgentConfig, AgentRunner, SingleSubmitAuxiliaryTool,
    SingleSubmitProfile, SingleSubmitTerminal,
};
use rollshot_agent::launch_teaser::{
    launch_teaser_run_budget, launch_teaser_submit_definition, SUBMIT_LAUNCH_TEASER_PLAN_TOOL_NAME,
};
use rollshot_agent::model::{ModelCompletion, ModelStreamEvent, ModelUsage, StopReason};
use rollshot_agent::product_task::{ProductTaskId, TaskAttemptId};
use rollshot_agent::repository::{repository_read_tool, RepositoryReadGrant, RepositoryReadLimits};
use rollshot_agent::runtime::RunCancellation;
use rollshot_agent::skills;

// ---- Test helpers (shared with driver tests) ----

fn tool_call_turn(id: &str, name: &str, args: &str) -> Vec<ModelStreamEvent> {
    vec![
        ModelStreamEvent::ToolCallStart {
            id: id.to_string(),
            name: name.to_string(),
        },
        ModelStreamEvent::ToolCallArgumentDelta {
            id: id.to_string(),
            delta: args.to_string(),
        },
        ModelStreamEvent::Completed(ModelCompletion {
            usage: ModelUsage {
                input_tokens: 5,
                output_tokens: 3,
                total_tokens: 8,
            },
            stop_reason: StopReason::ToolUse,
        }),
    ]
}

struct ScriptedProvider {
    scripts: std::sync::Mutex<std::collections::VecDeque<Vec<ModelStreamEvent>>>,
}

impl ScriptedProvider {
    fn new(scripts: Vec<Vec<ModelStreamEvent>>) -> Self {
        Self {
            scripts: std::sync::Mutex::new(std::collections::VecDeque::from(scripts)),
        }
    }
}

impl rollshot_agent::ProviderAdapter for ScriptedProvider {
    fn stream(
        &self,
        _request: rollshot_agent::model::ModelRequest,
        _bounds: rollshot_agent::StreamBounds,
    ) -> std::pin::Pin<
        Box<
            dyn std::future::Future<
                    Output = Result<
                        std::pin::Pin<
                            Box<
                                dyn futures_util::Stream<
                                        Item = Result<
                                            ModelStreamEvent,
                                            rollshot_agent::model::ModelError,
                                        >,
                                    > + Send,
                            >,
                        >,
                        rollshot_agent::model::ModelError,
                    >,
                > + Send
                + '_,
        >,
    > {
        let events = self.scripts.lock().unwrap().pop_front().unwrap_or_default();
        Box::pin(async move {
            let s = futures_util::stream::iter(events.into_iter().map(Ok));
            Ok(Box::pin(s)
                as std::pin::Pin<
                    Box<
                        dyn futures_util::Stream<
                                Item = Result<ModelStreamEvent, rollshot_agent::model::ModelError>,
                            > + Send,
                    >,
                >)
        })
    }
}

struct NoopAuditSink;

impl rollshot_agent::audit::AuditAppendSink for NoopAuditSink {
    fn append(
        &self,
        envelope: rollshot_agent::audit::AuditEnvelopeV1,
    ) -> std::pin::Pin<
        Box<
            dyn std::future::Future<
                    Output = Result<
                        rollshot_agent::audit::AuditAppendReceiptV1,
                        rollshot_agent::audit::AuditAppendError,
                    >,
                > + Send
                + '_,
        >,
    > {
        Box::pin(async move {
            Ok(rollshot_agent::audit::AuditAppendReceiptV1 {
                event_id: envelope.event_id().as_str().to_owned(),
                sequence: 1,
                record_hash: "test-record".to_owned(),
            })
        })
    }
}

fn runner() -> AgentRunner {
    AgentRunner::new(AgentConfig::default())
}

fn run_id() -> RunId {
    RunId::parse("run-00000000-0000-4000-8000-00000000002a").unwrap()
}

fn authority(grants: BTreeSet<RunOperation>) -> AuthoritySnapshot {
    let subject = AuthoritySubject::ActionGuideEphemeralGuide {
        guide_digest: "test-digest".to_string(),
    };
    AuthoritySnapshot::new(
        AuthorityBinding::new(
            ProductTaskId::parse("task-00000000-0000-4000-8000-000000000001").unwrap(),
            TaskAttemptId::new(1),
            run_id(),
            subject,
        ),
        "rollshot-v1".to_owned(),
        DisclosureCeiling::TextMetadataOnly,
        false,
        BTreeSet::new(),
        grants,
    )
    .unwrap()
}

fn subject() -> AuthoritySubject {
    AuthoritySubject::ActionGuideEphemeralGuide {
        guide_digest: "test-digest".to_string(),
    }
}

// ---- Contract fixtures ----

struct ContractFixture {
    root: tempfile::TempDir,
}

impl ContractFixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(
            root.path().join("README.md"),
            b"# Rollshot\nOfficial terminology.\n",
        )
        .unwrap();
        std::fs::write(root.path().join("secrets.env"), b"SECRET=abc\n").unwrap();
        Self { root }
    }
}

// ---- Acceptance tests ----

/// The model reads an authorized project file, then submits a patch.
#[tokio::test]
async fn authorized_repository_read_then_review_submission() {
    let fixture = ContractFixture::new();

    // Grant access to README.md only.
    let grant = RepositoryReadGrant::open(
        fixture.root.path(),
        vec!["README.md".into()],
        RepositoryReadLimits::v1(),
    )
    .unwrap();
    let reader = repository_read_tool(grant, RunCancellation::new());

    // Build profile with auxiliary tool.
    let skill_use = skills::bundled_action_guide_launch_teaser_use().unwrap();
    let prompt = compose_launch_teaser_prompt(&skill_use).unwrap();
    let profile = SingleSubmitProfile::from_skill(
        &skill_use,
        prompt,
        launch_teaser_submit_definition(),
        {
            // Create a terminal tool stub.
            struct TerminalStub;
            impl rollshot_agent::tools::Tool for TerminalStub {
                fn name(&self) -> &str {
                    SUBMIT_LAUNCH_TEASER_PLAN_TOOL_NAME
                }
                fn json_schema(&self) -> serde_json::Value {
                    serde_json::json!({"type": "object"})
                }
                fn call<'a>(
                    &'a self,
                    _arguments: &'a serde_json::Value,
                ) -> rollshot_agent::tools::ToolFuture<'a> {
                    Box::pin(async move {
                        Ok(rollshot_agent::tools::ToolOutcome::Success {
                            result_json: serde_json::json!({"submitted": true}),
                        })
                    })
                }
            }
            Arc::new(TerminalStub)
        },
        RunOperation::SubmitReviewCandidate,
        "rollshot::agent::launch_teaser",
    )
    .unwrap()
    .with_auxiliary_tools(vec![SingleSubmitAuxiliaryTool {
        definition: rollshot_agent::model::ToolDefinition {
            name: "read_authorized_project_text".to_string(),
            description: "Read authorized project text".to_string(),
            parameters: serde_json::json!({"type": "object"}),
        },
        tool: reader.tool(),
    }])
    .unwrap();

    // Scripted provider: first reads a file, then submits the patch.
    let patch = serde_json::json!({
        "hook": null,
        "outro_text": null,
        "shot_order": [1, 2, 3],
        "shots": [
            {"reviewed_step_id": 1},
            {"reviewed_step_id": 2},
            {"reviewed_step_id": 3}
        ]
    });
    let provider = ScriptedProvider::new(vec![
        tool_call_turn(
            "tc_read",
            "read_authorized_project_text",
            r#"{"path":"README.md"}"#,
        ),
        tool_call_turn(
            "tc_submit",
            SUBMIT_LAUNCH_TEASER_PLAN_TOOL_NAME,
            &patch.to_string(),
        ),
    ]);

    let input = rollshot_agent::domain::AuthorizedModelInput::new(
        "anthropic".into(),
        "model".into(),
        "improve teaser".into(),
        vec![],
        vec![],
    )
    .unwrap();

    let grants = BTreeSet::from([
        RunOperation::SubmitReviewCandidate,
        RunOperation::ReadAuthorizedWorkspaceFile,
    ]);
    let auth = authority(grants);
    let subj = subject();

    let terminal = runner()
        .run_single_submit_with_provider(
            profile,
            input,
            &provider,
            launch_teaser_run_budget(),
            &RunCancellation::new(),
            &auth,
            &subj,
            Some(&NoopAuditSink),
        )
        .await;

    // Verify terminal is Submitted with the patch.
    match &terminal {
        SingleSubmitTerminal::Submitted { arguments } => {
            assert_eq!(arguments["shot_order"], serde_json::json!([1, 2, 3]));
        }
        other => panic!("expected Submitted, got {other:?}"),
    }

    // Verify read receipts.
    let receipts = reader.receipts();
    assert_eq!(receipts.len(), 1);
    assert_eq!(receipts[0].relative_path, "README.md");
    assert!(!receipts[0].content_sha256.is_empty());

    // Verify no absolute paths leaked.
    let audit_text = format!("{terminal:?}");
    assert!(
        !audit_text.contains(fixture.root.path().to_str().unwrap()),
        "terminal Debug must not contain the absolute root path"
    );
}

/// Without a repository grant, the model sees no read tool and still submits.
#[tokio::test]
async fn no_repository_grant_still_submits() {
    let skill_use = skills::bundled_action_guide_launch_teaser_use().unwrap();
    let prompt = compose_launch_teaser_prompt(&skill_use).unwrap();

    struct TerminalStub;
    impl rollshot_agent::tools::Tool for TerminalStub {
        fn name(&self) -> &str {
            SUBMIT_LAUNCH_TEASER_PLAN_TOOL_NAME
        }
        fn json_schema(&self) -> serde_json::Value {
            serde_json::json!({"type": "object"})
        }
        fn call<'a>(
            &'a self,
            _arguments: &'a serde_json::Value,
        ) -> rollshot_agent::tools::ToolFuture<'a> {
            Box::pin(async move {
                Ok(rollshot_agent::tools::ToolOutcome::Success {
                    result_json: serde_json::json!({"submitted": true}),
                })
            })
        }
    }

    let profile = SingleSubmitProfile::from_skill(
        &skill_use,
        prompt,
        launch_teaser_submit_definition(),
        Arc::new(TerminalStub),
        RunOperation::SubmitReviewCandidate,
        "rollshot::agent::launch_teaser",
    )
    .unwrap();
    // No auxiliary tools.

    let patch = serde_json::json!({
        "hook": "Watch this",
        "outro_text": null,
        "shot_order": [10, 20, 30],
        "shots": [
            {"reviewed_step_id": 10},
            {"reviewed_step_id": 20},
            {"reviewed_step_id": 30}
        ]
    });
    let provider = ScriptedProvider::new(vec![tool_call_turn(
        "tc_submit",
        SUBMIT_LAUNCH_TEASER_PLAN_TOOL_NAME,
        &patch.to_string(),
    )]);

    let input = rollshot_agent::domain::AuthorizedModelInput::new(
        "anthropic".into(),
        "model".into(),
        "improve teaser".into(),
        vec![],
        vec![],
    )
    .unwrap();

    let grants = BTreeSet::from([RunOperation::SubmitReviewCandidate]);
    let auth = authority(grants);
    let subj = subject();

    let terminal = runner()
        .run_single_submit_with_provider(
            profile,
            input,
            &provider,
            launch_teaser_run_budget(),
            &RunCancellation::new(),
            &auth,
            &subj,
            Some(&NoopAuditSink),
        )
        .await;

    match &terminal {
        SingleSubmitTerminal::Submitted { arguments } => {
            assert_eq!(arguments["hook"], serde_json::json!("Watch this"));
            assert_eq!(arguments["shot_order"], serde_json::json!([10, 20, 30]));
        }
        other => panic!("expected Submitted, got {other:?}"),
    }
}
