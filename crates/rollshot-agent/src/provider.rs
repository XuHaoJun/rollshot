use std::collections::BTreeSet;
use std::pin::Pin;

use futures_util::{Stream, StreamExt};
use rig_core::agent::run::StreamedTurnAssembler;
use rig_core::client::CompletionClient;
use rig_core::completion::CompletionRequest;
use rig_core::message::{Message, UserContent};

use crate::model::{drive_streamed_turn, emit_tool_call_completions, ModelCompletion, StopReason};
use crate::model::{ModelError, ModelMessage, ModelRequest, ModelStreamEvent};
use crate::runtime::RunCancellation;

/// Snapshot of cancellation flag and deadline for bounding stream processing.
#[derive(Debug, Clone)]
pub struct StreamBounds {
    cancellation: RunCancellation,
    deadline: tokio::time::Instant,
}

impl StreamBounds {
    pub fn new(cancellation: RunCancellation, deadline: tokio::time::Instant) -> Self {
        Self {
            cancellation,
            deadline,
        }
    }
}

type StreamResult = Pin<Box<dyn Stream<Item = Result<ModelStreamEvent, ModelError>> + Send>>;

pub trait ProviderAdapter: Send + Sync {
    fn stream(
        &self,
        request: ModelRequest,
        bounds: StreamBounds,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<StreamResult, ModelError>> + Send + '_>>;
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

const DEFAULT_MAX_TOKENS: u64 = 4096;

impl AnthropicAdapter {
    pub fn new(api_key: &str, base_url: &str) -> Result<Self, ModelError> {
        let client = rig_core::providers::anthropic::Client::builder()
            .api_key(api_key)
            .base_url(base_url)
            .build()
            .map_err(|e| ModelError::ProviderFailure(format!("client build failed: {e}")))?;
        Ok(Self { client })
    }
}

impl ProviderAdapter for AnthropicAdapter {
    fn stream(
        &self,
        request: ModelRequest,
        bounds: StreamBounds,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<StreamResult, ModelError>> + Send + '_>>
    {
        Box::pin(async move {
            let tool_names: BTreeSet<String> = request
                .tool_definitions
                .iter()
                .map(|td| td.name.clone())
                .collect();

            let model_id = request.model.clone();
            let completion_request = build_completion_request(request)?;

            let model = self.client.completion_model(&model_id);
            use rig_core::completion::CompletionModel;
            let response = model
                .stream(completion_request)
                .await
                .map_err(rig_to_model_error)?;

            let output = stream_to_model_events(response, tool_names, bounds);
            Ok(Box::pin(output) as StreamResult)
        })
    }
}

fn build_completion_request(request: ModelRequest) -> Result<CompletionRequest, ModelError> {
    let mut chat_history: Vec<Message> = Vec::new();

    for msg in &request.history {
        chat_history.push(model_message_to_rig(msg));
    }

    // The prompt is empty when the full conversation (including the latest tool
    // result) is carried in `history`; only append it when present.
    if !request.prompt.is_empty() {
        chat_history.push(Message::user(&request.prompt));
    }

    if !request.attachments.is_empty() {
        let images = request
            .attachments
            .iter()
            .map(attachment_to_rig)
            .collect::<Vec<_>>();
        chat_history.push(Message::User {
            content: rig_core::OneOrMany::many(images)
                .map_err(|e| ModelError::ProtocolFailure(e.to_string()))?,
        });
    }

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
        model: Some(request.model),
        preamble: request.system_prompt,
        chat_history,
        documents: vec![],
        tools,
        temperature: None,
        max_tokens: Some(request.max_tokens.unwrap_or(DEFAULT_MAX_TOKENS)),
        tool_choice: None,
        additional_params: None,
        output_schema: None,
    })
}

fn model_message_to_rig(msg: &ModelMessage) -> Message {
    match msg {
        ModelMessage::User { content } => Message::user(content),
        ModelMessage::Assistant { content } => Message::assistant(content),
        ModelMessage::AssistantToolCall {
            id,
            name,
            arguments,
        } => Message::Assistant {
            id: None,
            content: rig_core::OneOrMany::one(rig_core::message::AssistantContent::ToolCall(
                rig_core::message::ToolCall::new(
                    id.clone(),
                    rig_core::message::ToolFunction::new(name.clone(), arguments.clone()),
                ),
            )),
        },
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

fn attachment_to_rig(attachment: &crate::model::ModelAttachment) -> UserContent {
    let media_type = match attachment.media_type() {
        crate::domain::MediaType::Png => rig_core::message::ImageMediaType::PNG,
        crate::domain::MediaType::Jpeg => rig_core::message::ImageMediaType::JPEG,
    };
    UserContent::image_raw(attachment.bytes().to_vec(), Some(media_type), None)
}

pub struct OpenAIAdapter {
    client: rig_core::providers::openai::CompletionsClient,
}

impl std::fmt::Debug for OpenAIAdapter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OpenAIAdapter")
            .field("client", &"<redacted>")
            .finish()
    }
}

impl OpenAIAdapter {
    pub fn new(api_key: &str, base_url: &str) -> Result<Self, ModelError> {
        let client = rig_core::providers::openai::Client::builder()
            .api_key(api_key)
            .base_url(base_url)
            .build()
            .map_err(|e| ModelError::ProviderFailure(format!("client build failed: {e}")))?
            .completions_api();
        Ok(Self { client })
    }
}

fn build_openai_completion_request(request: ModelRequest) -> Result<CompletionRequest, ModelError> {
    let mut req = build_completion_request(request)?;
    req.additional_params = Some(serde_json::json!({"parallel_tool_calls": false}));
    Ok(req)
}

impl ProviderAdapter for OpenAIAdapter {
    fn stream(
        &self,
        request: ModelRequest,
        bounds: StreamBounds,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<StreamResult, ModelError>> + Send + '_>>
    {
        Box::pin(async move {
            let tool_names: BTreeSet<String> = request
                .tool_definitions
                .iter()
                .map(|td| td.name.clone())
                .collect();

            let model_id = request.model.clone();
            let completion_request = build_openai_completion_request(request)?;

            let model = self.client.completion_model(&model_id);
            use rig_core::completion::CompletionModel;
            let response = model
                .stream(completion_request)
                .await
                .map_err(rig_to_model_error)?;

            let output = stream_to_model_events(response, tool_names, bounds);
            Ok(Box::pin(output) as StreamResult)
        })
    }
}

fn stream_to_model_events<R>(
    mut stream: rig_core::streaming::StreamingCompletionResponse<R>,
    tool_names: BTreeSet<String>,
    bounds: StreamBounds,
) -> impl Stream<Item = Result<ModelStreamEvent, ModelError>> + Send
where
    R: Clone + Unpin + rig_core::completion::GetTokenUsage + Send + 'static,
{
    let mut asm = StreamedTurnAssembler::new(tool_names.clone(), tool_names.clone());

    async_stream::stream! {
        let cancellation = bounds.cancellation.clone();
        let deadline = bounds.deadline;

        let mut completion: Option<ModelCompletion> = None;

        loop {
            if cancellation.is_cancelled() {
                yield Err(ModelError::StreamIncomplete("cancelled".into()));
                return;
            }

            tokio::select! {
                biased;
                _ = tokio::time::sleep_until(deadline) => {
                    yield Err(ModelError::StreamIncomplete("deadline exceeded".into()));
                    return;
                }
                item = stream.next() => {
                    match item {
                        Some(Ok(stream_item)) => {
                            let events = drive_streamed_turn(&mut asm, &stream_item);
                            match events {
                                Ok(bac_events) => {
                                    for event in &bac_events {
                                        match event {
                                            ModelStreamEvent::Completed(c) => {
                                                completion = Some(c.clone());
                                            }
                                            _ => {
                                                yield Ok(event.clone());
                                            }
                                        }
                                    }
                                }
                                Err(e) => {
                                    yield Err(e);
                                    return;
                                }
                            }
                        }
                        Some(Err(err)) => {
                            yield Err(rig_to_model_error(err));
                            return;
                        }
                        None => break,
                    }
                }
            }
            if completion.is_some() {
                break;
            }
        }

        let Some(mut completion) = completion else {
            yield Err(ModelError::StreamIncomplete(
                "provider stream ended before completion".to_string(),
            ));
            return;
        };

        // Rig synthesizes a Final response on bare EOF for both Anthropic and
        // OpenAI streams. The assembler converts Final into Completed with zero
        // usage when no proper stop signal was received. Check the accumulated
        // response usage to distinguish real completions from bare-EOF synthesis.
        let response_usage = stream.response.as_ref().map(|r| r.token_usage());
        let has_real_usage = response_usage.as_ref().is_some_and(|u| u.total_tokens > 0);

        let final_choice = rig_core::OneOrMany::one(
            rig_core::message::AssistantContent::text("")
        );
        let turn = asm.finish(None, &final_choice);

        let has_tool_calls = turn.choice.iter().any(|item| {
            matches!(item, rig_core::message::AssistantContent::ToolCall(_))
        });

        if !has_real_usage && !has_tool_calls {
            yield Err(ModelError::StreamIncomplete(
                "provider stream ended before completion".to_string(),
            ));
            return;
        }

        if has_tool_calls {
            completion.stop_reason = StopReason::ToolUse;
        }

        for event in emit_tool_call_completions(&turn) {
            yield Ok(event);
        }
        yield Ok(ModelStreamEvent::Completed(completion));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn image_request() -> ModelRequest {
        ModelRequest {
            model: "vision-model".into(),
            prompt: "Locate the target".into(),
            history: vec![],
            turn: 1,
            tool_definitions: vec![],
            system_prompt: None,
            max_tokens: Some(100),
            attachments: vec![crate::model::ModelAttachment::new(
                crate::domain::MediaType::Png,
                1,
                1,
                std::sync::Arc::from([1_u8, 2_u8]),
            )],
        }
    }

    fn assert_has_raw_png(request: CompletionRequest) {
        let last = request.chat_history.iter().last().expect("image message");
        let Message::User { content } = last else {
            panic!("last message must be user image")
        };
        assert!(matches!(
            content.iter().next().expect("image content"),
            UserContent::Image(rig_core::message::Image {
                data: rig_core::message::DocumentSourceKind::Raw(bytes),
                media_type: Some(rig_core::message::ImageMediaType::PNG),
                ..
            }) if bytes == &vec![1, 2]
        ));
    }

    #[test]
    fn provider_request_contains_image() {
        assert_has_raw_png(build_completion_request(image_request()).unwrap());
    }

    #[test]
    fn openai_provider_request_contains_image_and_serial_tool_calls() {
        let request = build_openai_completion_request(image_request()).unwrap();
        assert_eq!(
            request.additional_params,
            Some(serde_json::json!({"parallel_tool_calls": false}))
        );
        assert_has_raw_png(request);
    }

    #[test]
    fn text_only_request_preserves_previous_chat_history() {
        let request = ModelRequest {
            model: "m".into(),
            prompt: "hello".into(),
            history: vec![],
            turn: 1,
            tool_definitions: vec![],
            system_prompt: None,
            max_tokens: None,
            attachments: vec![],
        };
        let result = build_completion_request(request).unwrap();
        assert_eq!(result.chat_history.iter().count(), 1);
        let last = result.chat_history.iter().next().unwrap();
        let Message::User { content } = last else {
            panic!("expected user message")
        };
        assert!(matches!(
            content.iter().next().unwrap(),
            UserContent::Text(t) if t.text == "hello"
        ));
    }

    #[test]
    fn image_request_debug_redacts_raw_bytes() {
        let request = image_request();
        let debug = format!("{request:?}");
        assert!(
            !debug.contains("AQI="),
            "debug must not contain base64 of attachment bytes: {debug}"
        );
        assert!(
            !debug.contains("1, 2]"),
            "debug must not contain raw attachment bytes: {debug}"
        );
    }

    #[test]
    fn jpeg_attachment_maps_to_rig_jpeg() {
        let attachment = crate::model::ModelAttachment::new(
            crate::domain::MediaType::Jpeg,
            1,
            1,
            std::sync::Arc::from([0xff_u8, 0xd8_u8]),
        );
        assert!(matches!(
            attachment_to_rig(&attachment),
            UserContent::Image(rig_core::message::Image {
                media_type: Some(rig_core::message::ImageMediaType::JPEG),
                ..
            })
        ));
    }
}
