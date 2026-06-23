use std::collections::BTreeSet;
use std::pin::Pin;

use futures_util::{Stream, StreamExt};
use rig_core::agent::run::StreamedTurnAssembler;
use rig_core::client::CompletionClient;
use rig_core::completion::CompletionRequest;
use rig_core::message::{Message, UserContent};

use crate::model::{
    drive_streamed_turn, emit_tool_call_completions, ModelCompletion, ModelUsage, StopReason,
};
use crate::model::{ModelError, ModelMessage, ModelRequest, ModelStreamEvent};

type StreamResult = Pin<Box<dyn Stream<Item = Result<ModelStreamEvent, ModelError>> + Send>>;

pub trait ProviderAdapter: Send + Sync {
    fn stream(
        &self,
        request: ModelRequest,
    ) -> impl std::future::Future<Output = Result<StreamResult, ModelError>> + Send;
}

pub struct AnthropicAdapter {
    client: rig_core::providers::anthropic::Client,
}

impl std::fmt::Debug for AnthropicAdapter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AnthropicAdapter")
            .field("client", &"<redacted>")
            .finish()
    }
}

impl AnthropicAdapter {
    pub fn new(api_key: &str, base_url: &str) -> Self {
        let client = rig_core::providers::anthropic::Client::builder()
            .api_key(api_key)
            .base_url(base_url)
            .build()
            .expect("Anthropic client build failed");
        Self { client }
    }
}

impl ProviderAdapter for AnthropicAdapter {
    async fn stream(&self, request: ModelRequest) -> Result<StreamResult, ModelError> {
        let tool_names: BTreeSet<String> = request
            .tool_definitions
            .iter()
            .map(|td| td.name.clone())
            .collect();

        let completion_request = build_completion_request(request)?;

        let model = self.client.completion_model("claude-sonnet-4-6");
        use rig_core::completion::CompletionModel;
        let response = model
            .stream(completion_request)
            .await
            .map_err(rig_to_model_error)?;

        let output = stream_to_model_events(response, tool_names);
        Ok(Box::pin(output))
    }
}

fn build_completion_request(request: ModelRequest) -> Result<CompletionRequest, ModelError> {
    let mut chat_history: Vec<Message> = Vec::new();

    for msg in &request.history {
        chat_history.push(model_message_to_rig(msg));
    }

    chat_history.push(Message::user(&request.prompt));

    let chat_history = rig_core::OneOrMany::many(chat_history)
        .map_err(|e| ModelError::ProtocolFailure(e.to_string()))?;

    let tools: Vec<rig_core::completion::ToolDefinition> = request
        .tool_definitions
        .iter()
        .map(|td| rig_core::completion::ToolDefinition {
            name: td.name.clone(),
            description: td.description.clone(),
            parameters: td.parameters.clone(),
        })
        .collect();

    Ok(CompletionRequest {
        model: Some("claude-sonnet-4-6".to_string()),
        preamble: None,
        chat_history,
        documents: vec![],
        tools,
        temperature: None,
        max_tokens: Some(4096),
        tool_choice: None,
        additional_params: None,
        output_schema: None,
    })
}

fn model_message_to_rig(msg: &ModelMessage) -> Message {
    match msg {
        ModelMessage::User { content } => Message::user(content),
        ModelMessage::Assistant { content } => Message::assistant(content),
        ModelMessage::ToolResult {
            tool_call_id,
            result,
        } => {
            let tr = rig_core::message::ToolResultContent::from_tool_output(result.clone());
            Message::User {
                content: rig_core::OneOrMany::one(UserContent::tool_result(
                    tool_call_id.clone(),
                    tr,
                )),
            }
        }
    }
}

fn rig_to_model_error(err: rig_core::completion::CompletionError) -> ModelError {
    let msg = err.to_string();
    match &err {
        rig_core::completion::CompletionError::HttpError(_) => {
            ModelError::ProviderFailure(sanitize_error(&msg))
        }
        rig_core::completion::CompletionError::ResponseError(inner) => {
            if inner.contains("authentication") || inner.contains("Invalid API key") {
                ModelError::ProviderFailure(sanitize_error(inner))
            } else if inner.contains("rate_limit") || inner.contains("Rate limit") {
                ModelError::StreamIncomplete(sanitize_error(inner))
            } else {
                ModelError::ProtocolFailure(sanitize_error(inner))
            }
        }
        rig_core::completion::CompletionError::ProviderError(_) => {
            ModelError::ProviderFailure(sanitize_error(&msg))
        }
        rig_core::completion::CompletionError::JsonError(_) => {
            ModelError::ProtocolFailure(sanitize_error(&msg))
        }
        _ => ModelError::ProviderFailure(sanitize_error(&msg)),
    }
}

fn sanitize_error(msg: &str) -> String {
    if msg.len() > 500 {
        format!("{}...", &msg[..500])
    } else {
        msg.to_string()
    }
}

fn stream_to_model_events<R>(
    mut stream: rig_core::streaming::StreamingCompletionResponse<R>,
    tool_names: BTreeSet<String>,
) -> impl Stream<Item = Result<ModelStreamEvent, ModelError>> + Send
where
    R: Clone + Unpin + rig_core::completion::GetTokenUsage + Send + 'static,
{
    let mut asm = StreamedTurnAssembler::new(tool_names.clone(), tool_names.clone());

    async_stream::stream! {
        let mut saw_completed = false;

        while let Some(item) = stream.next().await {
            match item {
                Ok(stream_item) => {
                    let events = drive_streamed_turn(&mut asm, &stream_item);
                    match events {
                        Ok(bac_events) => {
                            for event in &bac_events {
                                if matches!(event, ModelStreamEvent::Completed(_)) {
                                    saw_completed = true;
                                }
                                yield Ok(event.clone());
                            }
                        }
                        Err(e) => {
                            yield Err(e);
                            return;
                        }
                    }
                }
                Err(err) => {
                    yield Err(rig_to_model_error(err));
                    return;
                }
            }
        }

        // If the stream ended without a Final (common in Anthropic streaming
        // because Rig's SSE loop breaks on message_delta with stop_reason),
        // finish the assembler and emit tool-call completions + a synthetic
        // Completed event so downstream consumers always see a terminal event.
        if !saw_completed {
            let final_choice = rig_core::OneOrMany::one(
                rig_core::message::AssistantContent::text("")
            );
            let turn = asm.finish(None, &final_choice);

            // Infer stop reason from assembled content
            let has_tool_calls = turn.choice.iter().any(|c| {
                matches!(c, rig_core::message::AssistantContent::ToolCall(_))
            });
            let stop_reason = if has_tool_calls {
                StopReason::ToolUse
            } else {
                StopReason::EndTurn
            };

            let completions = emit_tool_call_completions(&turn);
            for event in completions {
                yield Ok(event);
            }

            // Emit a synthetic Completed — usage was consumed by Rig's
            // internal aggregation so we emit zero here.
            yield Ok(ModelStreamEvent::Completed(
                ModelCompletion {
                    usage: ModelUsage::default(),
                    stop_reason,
                },
            ));
        }
    }
}
