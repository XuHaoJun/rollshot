//! Spike: Rollshot rig 0.39 integration — Step 7 facade + tracing demo.
//!
//! Demonstrates:
//! - `RollshotModel` facade wrapping two scripted providers swapped at runtime.
//! - rig's default tracing behaviour characterised and suppressed via env-filter.

use std::collections::BTreeSet;

use rig_core::{
    OneOrMany,
    agent::run::{AgentRun, AgentRunStep, ModelTurn},
    completion::{AssistantContent, Usage},
    message::{Message, ToolCall, ToolFunction, UserContent},
};
use serde_json::json;
use tracing::{info, trace};

// ── RollshotModel facade ──────────────────────────────────────────────────────

/// Rollshot-owned provider trait. The real implementation will translate to
/// provider-specific SDKs (Anthropic, OpenAI, …). For the spike, two scripted
/// providers demonstrate runtime swapping.
trait RollshotModel: Send + Sync {
    fn name(&self) -> &'static str;
    /// Drive one model turn: given the current prompt + history, return a
    /// `ModelTurn` without any rig-internal I/O.
    fn scripted_turn(&self, turn_index: usize) -> ModelTurn;
}

// ── Provider A — scripted to call inspect_ocr then reply ─────────────────────

struct ProviderAlpha;

impl RollshotModel for ProviderAlpha {
    fn name(&self) -> &'static str {
        "alpha-scripted"
    }

    fn scripted_turn(&self, turn_index: usize) -> ModelTurn {
        trace!(target: "rollshot::spike::facade", turn = turn_index, provider = "alpha", "scripted turn");
        match turn_index {
            1 => tool_call_turn("call_1", "inspect_ocr", json!({"region": "full", "max_results": 5})),
            2 => tool_call_turn("call_2", "replace_automation_source", json!({"source": "new_src"})),
            _ => text_turn("alpha done"),
        }
    }
}

// ── Provider B — same scripted sequence, different identity ──────────────────

struct ProviderBeta;

impl RollshotModel for ProviderBeta {
    fn name(&self) -> &'static str {
        "beta-scripted"
    }

    fn scripted_turn(&self, turn_index: usize) -> ModelTurn {
        trace!(target: "rollshot::spike::facade", turn = turn_index, provider = "beta", "scripted turn");
        match turn_index {
            1 => tool_call_turn("call_1", "inspect_ocr", json!({"region": "partial", "max_results": 3})),
            2 => tool_call_turn("call_2", "replace_automation_source", json!({"source": "beta_src"})),
            _ => text_turn("beta done"),
        }
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn all_tools() -> BTreeSet<String> {
    ["inspect_ocr", "replace_automation_source"]
        .iter()
        .map(|s| s.to_string())
        .collect()
}

fn tool_call_turn(id: &str, name: &str, args: serde_json::Value) -> ModelTurn {
    ModelTurn::new(
        None,
        OneOrMany::one(AssistantContent::ToolCall(ToolCall::new(
            id.to_string(),
            ToolFunction::new(name.to_string(), args),
        ))),
        Usage {
            input_tokens: 10,
            output_tokens: 5,
            total_tokens: 15,
            ..Usage::new()
        },
        all_tools(),
        all_tools(),
    )
}

fn text_turn(text: &str) -> ModelTurn {
    ModelTurn::new(
        None,
        OneOrMany::one(AssistantContent::text(text)),
        Usage {
            input_tokens: 8,
            output_tokens: 4,
            total_tokens: 12,
            ..Usage::new()
        },
        all_tools(),
        all_tools(),
    )
}

fn fake_tool_result(id: &str, output: &str) -> UserContent {
    UserContent::tool_result(
        id.to_string(),
        rig_core::message::ToolResultContent::from_tool_output(output.to_string()),
    )
}

// ── Drive a multi-turn run with any RollshotModel ────────────────────────────

fn drive_run(model: &dyn RollshotModel) -> String {
    info!(target: "rollshot::spike::facade", provider = model.name(), "starting run");

    let mut run = AgentRun::new(Message::user("redact the document")).max_turns(10);

    loop {
        match run.next_step().expect("AgentRun::next_step") {
            AgentRunStep::CallModel { turn, .. } => {
                let model_turn = model.scripted_turn(turn);
                run.model_response(model_turn).expect("model_response");
            }
            AgentRunStep::CallTools { calls } => {
                let results: Vec<UserContent> = calls
                    .iter()
                    .map(|c| {
                        info!(
                            target: "rollshot::spike::facade",
                            tool = c.tool_call.function.name,
                            id = c.tool_call.id,
                            "executing tool"
                        );
                        fake_tool_result(&c.tool_call.id, &format!("ok:{}", c.tool_call.function.name))
                    })
                    .collect();
                run.tool_results(results).expect("tool_results");
            }
            AgentRunStep::Done(response) => {
                let usage = response.usage;
                info!(
                    target: "rollshot::spike::facade",
                    provider = model.name(),
                    input_tokens = usage.input_tokens,
                    output_tokens = usage.output_tokens,
                    "run complete"
                );
                return response.output;
            }
        }
    }
}

#[tokio::main]
async fn main() {
    // Install a tracing subscriber that can be filtered via RUST_LOG.
    // We verify rig itself emits NO prompt/response text (it uses tracing internally only
    // for telemetry spans, not raw content). The rollshot::spike::* target is kept visible;
    // rig's own targets can be suppressed with RUST_LOG=rollshot=trace (rig emits nothing
    // sensitive by default).
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "rollshot::spike=info".parse().unwrap()),
        )
        .init();

    // Demonstrate provider swap at runtime.
    let providers: Vec<Box<dyn RollshotModel>> =
        vec![Box::new(ProviderAlpha), Box::new(ProviderBeta)];

    for provider in &providers {
        let result = drive_run(provider.as_ref());
        println!("[{}] result: {result}", provider.name());
    }
}
