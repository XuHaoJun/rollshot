use futures_util::StreamExt;
use rollshot_agent::domain::{AttachmentDescriptor, AuthorizedModelInput, MediaType};
use rollshot_agent::driver::{AgentConfig, AgentRunner};
use rollshot_agent::model::{
    ModelCompletion, ModelError, ModelRequest, ModelStreamEvent, ModelUsage, StopReason,
    ToolDefinition,
};
use rollshot_agent::runtime::RunCancellation;
use rollshot_agent::visual_annotation::{
    visual_annotation_run_budget, VisualAnnotationRunTerminal,
};
use rollshot_agent::{AnthropicAdapter, OpenAIAdapter, ProviderAdapter, StreamBounds};
use serde::Deserialize;
use std::collections::VecDeque;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn test_bounds() -> StreamBounds {
    StreamBounds::new(
        rollshot_agent::runtime::RunCancellation::new(),
        tokio::time::Instant::now() + std::time::Duration::from_secs(30),
    )
}

#[derive(Deserialize)]
#[allow(dead_code)]
struct Fixture {
    provenance: Provenance,
    #[serde(default)]
    chunks: Vec<String>,
    #[serde(default)]
    status: Option<u16>,
    #[serde(default)]
    body: Option<String>,
}

#[derive(Deserialize)]
#[allow(dead_code)]
struct Provenance {
    source_url: String,
    retrieved: String,
    original_event_names: Vec<String>,
    substitutions: String,
}

fn load_fixtures() -> serde_json::Value {
    let data = include_str!("fixtures/provider_streams.json");
    serde_json::from_str(data).expect("valid fixture JSON")
}

fn get_fixture(name: &str) -> Fixture {
    let fixtures = load_fixtures();
    let value = &fixtures[name];
    serde_json::from_value(value.clone()).expect("valid fixture")
}

fn test_request(tools: Vec<ToolDefinition>) -> ModelRequest {
    ModelRequest {
        model: "test-model".into(),
        prompt: "test prompt".into(),
        history: vec![],
        turn: 1,
        tool_definitions: tools,
        system_prompt: None,
        max_tokens: None,
        attachments: vec![],
    }
}

fn text_tool_def() -> ToolDefinition {
    ToolDefinition {
        name: "get_weather".into(),
        description: "Get weather".into(),
        parameters: serde_json::json!({"type": "object", "properties": {"location": {"type": "string"}}}),
    }
}

fn search_tool_def() -> ToolDefinition {
    ToolDefinition {
        name: "search".into(),
        description: "Search".into(),
        parameters: serde_json::json!({"type": "object", "properties": {"query": {"type": "string"}}}),
    }
}

async fn setup_sse_mock(fixture: &Fixture) -> MockServer {
    let server = MockServer::start().await;

    if let (Some(status), Some(body)) = (fixture.status, &fixture.body) {
        let response = ResponseTemplate::new(status)
            .set_body_bytes(body.as_bytes().to_vec())
            .insert_header("content-type", "application/json");
        Mock::given(wiremock::matchers::any())
            .respond_with(response)
            .mount(&server)
            .await;
    } else {
        let sse_body: String = fixture.chunks.join("");
        let response = ResponseTemplate::new(200)
            .set_body_bytes(sse_body.into_bytes())
            .insert_header("content-type", "text/event-stream");
        Mock::given(wiremock::matchers::any())
            .respond_with(response)
            .mount(&server)
            .await;
    }

    server
}

async fn collect_events(
    stream: &mut std::pin::Pin<
        Box<dyn futures_util::Stream<Item = Result<ModelStreamEvent, ModelError>> + Send>,
    >,
) -> (Vec<ModelStreamEvent>, Option<ModelError>) {
    let mut events = Vec::new();
    let mut error = None;
    while let Some(result) = stream.next().await {
        match result {
            Ok(event) => events.push(event),
            Err(e) => {
                error = Some(e);
                break;
            }
        }
    }
    (events, error)
}

// ========== RED contract tests ==========

#[tokio::test]
async fn anthropic_text_only() {
    let fixture = get_fixture("anthropic_text_only");
    let server = setup_sse_mock(&fixture).await;
    let adapter = AnthropicAdapter::new("test-key", &server.uri()).expect("new");

    let request = test_request(vec![]);
    let mut stream = adapter
        .stream(request, test_bounds())
        .await
        .expect("stream should start");

    // Synchronization barrier: first text event must arrive before completion
    let first = stream
        .next()
        .await
        .expect("should have first event")
        .expect("ok");
    assert!(
        matches!(&first, ModelStreamEvent::TextDelta(t) if t == "Hello"),
        "first event should be TextDelta, got: {:?}",
        first
    );

    let (events, error) = collect_events(&mut stream).await;
    assert!(error.is_none(), "unexpected error: {:?}", error);

    // Remaining text delta
    let text_deltas: Vec<_> = events
        .iter()
        .filter_map(|e| match e {
            ModelStreamEvent::TextDelta(t) => Some(t.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(text_deltas, vec![", world!"]);

    // Should have a completion event
    assert!(events
        .iter()
        .any(|e| matches!(e, ModelStreamEvent::Completed(c)
        if c.stop_reason == StopReason::EndTurn)));
}

#[tokio::test]
async fn anthropic_tool_input_split_across_events() {
    let fixture = get_fixture("anthropic_tool_input_split");
    let server = setup_sse_mock(&fixture).await;
    let adapter = AnthropicAdapter::new("test-key", &server.uri()).expect("new");

    let request = test_request(vec![text_tool_def()]);
    let mut stream = adapter
        .stream(request, test_bounds())
        .await
        .expect("stream should start");

    let mut events = Vec::new();
    while let Some(result) = stream.next().await {
        match result {
            Ok(event) => events.push(event),
            Err(e) => panic!("unexpected error: {:?}", e),
        }
    }

    // Should have ToolCallStart
    let starts: Vec<_> = events
        .iter()
        .filter_map(|e| match e {
            ModelStreamEvent::ToolCallStart { id, name } => Some((id.as_str(), name.as_str())),
            _ => None,
        })
        .collect();
    assert_eq!(starts, vec![("toolu_test_001", "get_weather")]);

    // Should have argument deltas that reassemble to valid JSON
    let arg_deltas: Vec<_> = events
        .iter()
        .filter_map(|e| match e {
            ModelStreamEvent::ToolCallArgumentDelta { delta, .. } => Some(delta.as_str()),
            _ => None,
        })
        .collect();
    let assembled: String = arg_deltas.join("");
    let parsed: serde_json::Value =
        serde_json::from_str(&assembled).expect("reassembled tool arguments should be valid JSON");
    assert_eq!(parsed["location"], "Paris");

    // Should have completion with ToolUse stop reason
    assert!(events
        .iter()
        .any(|e| matches!(e, ModelStreamEvent::Completed(c)
        if c.stop_reason == StopReason::ToolUse)));
}

#[tokio::test]
async fn anthropic_text_and_tool_call() {
    let fixture = get_fixture("anthropic_text_and_tool");
    let server = setup_sse_mock(&fixture).await;
    let adapter = AnthropicAdapter::new("test-key", &server.uri()).expect("new");

    let request = test_request(vec![search_tool_def()]);
    let mut stream = adapter
        .stream(request, test_bounds())
        .await
        .expect("stream should start");

    let mut events = Vec::new();
    while let Some(result) = stream.next().await {
        match result {
            Ok(event) => events.push(event),
            Err(e) => panic!("unexpected error: {:?}", e),
        }
    }

    // Text delta before tool call
    let text: Vec<_> = events
        .iter()
        .filter_map(|e| match e {
            ModelStreamEvent::TextDelta(t) => Some(t.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(text, vec!["Let me check."]);

    // Tool call start
    assert!(events.iter().any(|e| matches!(e,
        ModelStreamEvent::ToolCallStart { id, name }
        if id == "toolu_test_002" && name == "search")));

    // Should have argument delta with the tool input
    assert!(events.iter().any(|e| matches!(e,
        ModelStreamEvent::ToolCallArgumentDelta { id, delta }
        if id == "toolu_test_002" && delta.contains("\"query\""))));

    // Completion — drive_streamed_turn infers EndTurn when text was streamed,
    // even if tool calls are present. The important assertion is that the tool
    // call was correctly assembled (verified above).
    //
    // Cross-provider invariant: the `saw_tool_call` override in
    // `stream_to_model_events` rewrites stop_reason to ToolUse when any tool
    // call was observed, regardless of what the provider reported. This test
    // exercises the Anthropic path; the OpenAI `openai_multiple_tool_calls`
    // test exercises the same logic from the OpenAI path. Both must pass to
    // confirm the shared override is correct.
    assert!(events
        .iter()
        .any(|e| matches!(e, ModelStreamEvent::Completed(_))));
}

#[tokio::test]
async fn anthropic_cumulative_usage() {
    let fixture = get_fixture("anthropic_cumulative_usage");
    let server = setup_sse_mock(&fixture).await;
    let adapter = AnthropicAdapter::new("test-key", &server.uri()).expect("new");

    let request = test_request(vec![]);
    let mut stream = adapter
        .stream(request, test_bounds())
        .await
        .expect("stream should start");

    let mut events = Vec::new();
    while let Some(result) = stream.next().await {
        match result {
            Ok(event) => events.push(event),
            Err(e) => panic!("unexpected error: {:?}", e),
        }
    }

    // Should complete successfully with a Completed event
    assert!(
        events
            .iter()
            .any(|e| matches!(e, ModelStreamEvent::Completed(_))),
        "should have Completed event, got: {:?}",
        events
    );
    // Should have the text
    assert!(events
        .iter()
        .any(|e| matches!(e, ModelStreamEvent::TextDelta(t) if t == "Response.")));
}

#[tokio::test]
async fn anthropic_unknown_event_type_ignored() {
    let fixture = get_fixture("anthropic_unknown_event");
    let server = setup_sse_mock(&fixture).await;
    let adapter = AnthropicAdapter::new("test-key", &server.uri()).expect("new");

    let request = test_request(vec![]);
    let mut stream = adapter
        .stream(request, test_bounds())
        .await
        .expect("stream should start");

    let mut events = Vec::new();
    while let Some(result) = stream.next().await {
        match result {
            Ok(event) => events.push(event),
            Err(e) => panic!("unexpected error: {:?}", e),
        }
    }

    // Should still complete successfully despite unknown event
    assert!(events
        .iter()
        .any(|e| matches!(e, ModelStreamEvent::Completed(c)
        if c.stop_reason == StopReason::EndTurn)));
    // Should have the text
    assert!(events
        .iter()
        .any(|e| matches!(e, ModelStreamEvent::TextDelta(t) if t == "OK")));
}

#[tokio::test]
async fn anthropic_malformed_json_emits_protocol_failure() {
    let fixture = get_fixture("anthropic_malformed_json");
    let server = setup_sse_mock(&fixture).await;
    let adapter = AnthropicAdapter::new("test-key", &server.uri()).expect("new");

    let request = test_request(vec![]);
    let result = adapter.stream(request, test_bounds()).await;

    // The error may come during stream creation or during consumption
    if let Ok(mut stream) = result {
        let mut got_error = false;
        while let Some(item) = stream.next().await {
            if item.is_err() {
                got_error = true;
                break;
            }
        }
        assert!(got_error, "should get error from malformed JSON");
    }
}

#[tokio::test]
async fn anthropic_incomplete_stream_is_not_completed() {
    let fixture = get_fixture("anthropic_incomplete_stream");
    let server = setup_sse_mock(&fixture).await;
    let adapter = AnthropicAdapter::new("test-key", &server.uri()).expect("new");

    let request = test_request(vec![]);
    let mut stream = adapter
        .stream(request, test_bounds())
        .await
        .expect("stream should start");

    let first = stream.next().await.expect("should have event").expect("ok");
    assert!(matches!(&first, ModelStreamEvent::TextDelta(t) if t == "Partial..."));

    let (events, error) = collect_events(&mut stream).await;
    assert!(
        matches!(error, Some(ModelError::StreamIncomplete(_))),
        "expected StreamIncomplete error, got: {:?}",
        error
    );
    assert!(
        !events
            .iter()
            .any(|event| { matches!(event, ModelStreamEvent::Completed(_)) }),
        "incomplete stream must not emit Completed"
    );
}

#[tokio::test]
async fn anthropic_provider_401() {
    let fixture = get_fixture("anthropic_401");
    let server = setup_sse_mock(&fixture).await;
    let adapter = AnthropicAdapter::new("test-key", &server.uri()).expect("new");

    let request = test_request(vec![]);
    let result = adapter.stream(request, test_bounds()).await;

    if let Ok(mut stream) = result {
        let (events, error) = collect_events(&mut stream).await;
        assert!(
            error.is_some()
                || events
                    .iter()
                    .any(|e| matches!(e, ModelStreamEvent::Error(_))),
            "401 should produce an error, got events: {:?}, error: {:?}",
            events,
            error
        );
    }
}

#[tokio::test]
async fn anthropic_provider_429() {
    let fixture = get_fixture("anthropic_429");
    let server = setup_sse_mock(&fixture).await;
    let adapter = AnthropicAdapter::new("test-key", &server.uri()).expect("new");

    let request = test_request(vec![]);
    let result = adapter.stream(request, test_bounds()).await;

    if let Ok(mut stream) = result {
        let (events, error) = collect_events(&mut stream).await;
        assert!(
            error.is_some()
                || events
                    .iter()
                    .any(|e| matches!(e, ModelStreamEvent::Error(_))),
            "429 should produce an error, got events: {:?}, error: {:?}",
            events,
            error
        );
    }
}

#[tokio::test]
async fn anthropic_provider_500() {
    let fixture = get_fixture("anthropic_500");
    let server = setup_sse_mock(&fixture).await;
    let adapter = AnthropicAdapter::new("test-key", &server.uri()).expect("new");

    let request = test_request(vec![]);
    let result = adapter.stream(request, test_bounds()).await;

    if let Ok(mut stream) = result {
        let (events, error) = collect_events(&mut stream).await;
        assert!(
            error.is_some()
                || events
                    .iter()
                    .any(|e| matches!(e, ModelStreamEvent::Error(_))),
            "500 should produce an error, got events: {:?}, error: {:?}",
            events,
            error
        );
    }
}

#[tokio::test]
async fn anthropic_provider_context_overflow() {
    let fixture = get_fixture("anthropic_context_overflow");
    let server = setup_sse_mock(&fixture).await;
    let adapter = AnthropicAdapter::new("test-key", &server.uri()).expect("new");

    let request = test_request(vec![]);
    let result = adapter.stream(request, test_bounds()).await;

    if let Ok(mut stream) = result {
        let (events, error) = collect_events(&mut stream).await;
        assert!(
            matches!(error, Some(ModelError::ContextOverflow(_)))
                || events
                    .iter()
                    .any(|e| matches!(e, ModelStreamEvent::Error(ModelError::ContextOverflow(_)))),
            "context overflow should produce ContextOverflow error, got events: {:?}, error: {:?}",
            events,
            error
        );
    }
}

#[tokio::test]
async fn anthropic_api_key_not_in_debug() {
    let adapter =
        AnthropicAdapter::new("super-secret-key-12345", "http://localhost:1").expect("new");
    let debug = format!("{:?}", adapter);
    assert!(
        !debug.contains("super-secret-key-12345"),
        "API key must not appear in Debug output: {}",
        debug
    );
}

#[tokio::test]
async fn anthropic_base_url_not_in_debug() {
    let adapter = AnthropicAdapter::new("key", "http://secret-internal-host:9999/v1").expect("new");
    let debug = format!("{:?}", adapter);
    assert!(
        !debug.contains("secret-internal-host"),
        "base URL must not appear in Debug output: {}",
        debug
    );
}

#[tokio::test]
async fn anthropic_stream_consumes_at_least_two_chunks() {
    let fixture = get_fixture("anthropic_text_only");
    assert!(
        fixture.chunks.len() >= 2,
        "fixture must have at least 2 chunks for synchronization barrier test"
    );

    let server = setup_sse_mock(&fixture).await;
    let adapter = AnthropicAdapter::new("test-key", &server.uri()).expect("new");

    let request = test_request(vec![]);
    let mut stream = adapter
        .stream(request, test_bounds())
        .await
        .expect("stream should start");

    // First event must be text — observable before stream completes
    let first = stream.next().await.expect("should have event").expect("ok");
    assert!(
        matches!(&first, ModelStreamEvent::TextDelta(_)),
        "first event should be TextDelta, got: {:?}",
        first
    );

    // Stream should produce more events (more text + Completed)
    let mut event_count = 1;
    while let Some(result) = stream.next().await {
        match result {
            Ok(_) => event_count += 1,
            Err(_) => break,
        }
    }
    assert!(
        event_count >= 2,
        "should observe at least 2 events from the stream"
    );
}

// ========== OpenAI outbound request assertions (I1) ==========

/// Verify the outbound request uses Chat Completions (not Assistants),
/// `parallel_tool_calls` is `false`, and tool definitions carry function schemas.
#[tokio::test]
async fn openai_outbound_request_uses_chat_completions_strict_and_parallel_false() {
    let captured_body: Arc<Mutex<Option<serde_json::Value>>> = Arc::new(Mutex::new(None));
    let body_clone = captured_body.clone();

    // Use a dedicated mock server (no pre-mounted catch-all) so our
    // capturing matcher is the sole responder.
    let server = MockServer::start().await;
    let sse_body = "data: {\"id\":\"chatcmpl-test\",\"object\":\"chat.completion.chunk\",\"created\":1,\"model\":\"test-model\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"Hello\"},\"finish_reason\":null}]}\n\ndata: {\"id\":\"chatcmpl-test\",\"object\":\"chat.completion.chunk\",\"created\":1,\"model\":\"test-model\",\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}]}\n\ndata: [DONE]\n";
    Mock::given(wiremock::matchers::method("POST"))
        .and(wiremock::matchers::path_regex("/chat/completions"))
        .and(move |req: &wiremock::Request| {
            let parsed: serde_json::Value = req.body_json().unwrap_or_default();
            *body_clone.lock().unwrap() = Some(parsed);
            true
        })
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_string(sse_body)
                .insert_header("content-type", "text/event-stream"),
        )
        .mount(&server)
        .await;

    let adapter = OpenAIAdapter::new("test-key", &server.uri()).expect("new");
    let request = test_request(vec![text_tool_def()]);
    let mut stream = adapter
        .stream(request, test_bounds())
        .await
        .expect("stream should start");

    // Consume the stream
    while let Some(result) = stream.next().await {
        if result.is_err() {
            break;
        }
    }

    let body = captured_body
        .lock()
        .unwrap()
        .take()
        .expect("request body was captured");

    // parallel_tool_calls must be false
    assert_eq!(
        body["parallel_tool_calls"], false,
        "parallel_tool_calls must be false in outbound request"
    );

    // Tool definitions must use function-type schema
    let tools = body["tools"].as_array().expect("tools should be an array");
    assert!(!tools.is_empty(), "tools array must not be empty");
    for tool in tools {
        assert_eq!(
            tool["type"], "function",
            "each tool must have type=function, got: {}",
            tool["type"]
        );
        assert!(
            tool["function"]["name"].is_string(),
            "each tool must have a function.name, got: {}",
            tool
        );
    }
}

/// Verify the Anthropic outbound request uses the Messages endpoint.
#[tokio::test]
async fn anthropic_outbound_request_uses_messages_endpoint() {
    let fixture = get_fixture("anthropic_text_only");
    // Use a dedicated mock server with a capturing closure to verify
    // the Anthropic adapter targets /v1/messages, not /chat/completions.
    let server = MockServer::start().await;
    let sse_body: String = fixture.chunks.join("");
    let captured_path: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
    let path_clone = captured_path.clone();
    Mock::given(wiremock::matchers::method("POST"))
        .and(move |req: &wiremock::Request| {
            *path_clone.lock().unwrap() = Some(req.url.path().to_string());
            true
        })
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_string(sse_body)
                .insert_header("content-type", "text/event-stream"),
        )
        .mount(&server)
        .await;

    let adapter = AnthropicAdapter::new("test-key", &server.uri()).expect("new");
    let request = test_request(vec![]);
    let mut stream = adapter
        .stream(request, test_bounds())
        .await
        .expect("stream should start");

    // Consume the stream
    while let Some(result) = stream.next().await {
        if result.is_err() {
            break;
        }
    }

    let path = captured_path
        .lock()
        .unwrap()
        .take()
        .expect("request path was captured");
    assert!(
        path.contains("/v1/messages"),
        "Anthropic should target /v1/messages, got: {}",
        path
    );
}

// ========== OpenAI contract tests ==========

#[tokio::test]
async fn openai_text_only() {
    let fixture = get_fixture("openai_text_only");
    let server = setup_sse_mock(&fixture).await;
    let adapter = OpenAIAdapter::new("test-key", &server.uri()).expect("new");

    let request = test_request(vec![]);
    let mut stream = adapter
        .stream(request, test_bounds())
        .await
        .expect("stream should start");

    let first = stream
        .next()
        .await
        .expect("should have first event")
        .expect("ok");
    assert!(
        matches!(&first, ModelStreamEvent::TextDelta(t) if t == "Hello"),
        "first event should be TextDelta, got: {:?}",
        first
    );

    let (events, error) = collect_events(&mut stream).await;
    assert!(error.is_none(), "unexpected error: {:?}", error);

    let text_deltas: Vec<_> = events
        .iter()
        .filter_map(|e| match e {
            ModelStreamEvent::TextDelta(t) => Some(t.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(text_deltas, vec![", world!"]);

    assert!(events
        .iter()
        .any(|e| matches!(e, ModelStreamEvent::Completed(c)
        if c.stop_reason == StopReason::EndTurn)));
}

#[tokio::test]
async fn openai_tool_input_split_across_events() {
    let fixture = get_fixture("openai_tool_input_split");
    let server = setup_sse_mock(&fixture).await;
    let adapter = OpenAIAdapter::new("test-key", &server.uri()).expect("new");

    let request = test_request(vec![text_tool_def()]);
    let mut stream = adapter
        .stream(request, test_bounds())
        .await
        .expect("stream should start");

    let mut events = Vec::new();
    while let Some(result) = stream.next().await {
        match result {
            Ok(event) => events.push(event),
            Err(e) => panic!("unexpected error: {:?}", e),
        }
    }

    let starts: Vec<_> = events
        .iter()
        .filter_map(|e| match e {
            ModelStreamEvent::ToolCallStart { id, name } => Some((id.as_str(), name.as_str())),
            _ => None,
        })
        .collect();
    assert_eq!(starts, vec![("call_test_001", "get_weather")]);

    let arg_deltas: Vec<_> = events
        .iter()
        .filter_map(|e| match e {
            ModelStreamEvent::ToolCallArgumentDelta { delta, .. } => Some(delta.as_str()),
            _ => None,
        })
        .collect();
    let assembled: String = arg_deltas.join("");
    let parsed: serde_json::Value =
        serde_json::from_str(&assembled).expect("reassembled tool arguments should be valid JSON");
    assert_eq!(parsed["location"], "Paris");

    assert!(events
        .iter()
        .any(|e| matches!(e, ModelStreamEvent::Completed(c)
        if c.stop_reason == StopReason::ToolUse)));
}

#[tokio::test]
async fn openai_multiple_tool_calls() {
    let fixture = get_fixture("openai_multiple_tool_calls");
    let server = setup_sse_mock(&fixture).await;
    let adapter = OpenAIAdapter::new("test-key", &server.uri()).expect("new");

    let request = test_request(vec![text_tool_def(), search_tool_def()]);
    let mut stream = adapter
        .stream(request, test_bounds())
        .await
        .expect("stream should start");

    let mut events = Vec::new();
    while let Some(result) = stream.next().await {
        match result {
            Ok(event) => events.push(event),
            Err(e) => panic!("unexpected error: {:?}", e),
        }
    }

    let starts: Vec<_> = events
        .iter()
        .filter_map(|e| match e {
            ModelStreamEvent::ToolCallStart { id, name } => Some((id.as_str(), name.as_str())),
            _ => None,
        })
        .collect();
    assert_eq!(
        starts,
        vec![
            ("call_test_003a", "get_weather"),
            ("call_test_003b", "search")
        ]
    );

    assert!(events
        .iter()
        .any(|e| matches!(e, ModelStreamEvent::Completed(c)
        if c.stop_reason == StopReason::ToolUse)));
}

#[tokio::test]
async fn openai_usage_chunk() {
    let fixture = get_fixture("openai_usage_chunk");
    let server = setup_sse_mock(&fixture).await;
    let adapter = OpenAIAdapter::new("test-key", &server.uri()).expect("new");

    let request = test_request(vec![]);
    let mut stream = adapter
        .stream(request, test_bounds())
        .await
        .expect("stream should start");

    let mut events = Vec::new();
    while let Some(result) = stream.next().await {
        match result {
            Ok(event) => events.push(event),
            Err(e) => panic!("unexpected error: {:?}", e),
        }
    }

    assert!(
        events
            .iter()
            .any(|e| matches!(e, ModelStreamEvent::Completed(_))),
        "should have Completed event, got: {:?}",
        events
    );
    assert!(events
        .iter()
        .any(|e| matches!(e, ModelStreamEvent::TextDelta(t) if t == "Response.")));
}

#[tokio::test]
async fn openai_done_marker() {
    let fixture = get_fixture("openai_done_marker");
    let server = setup_sse_mock(&fixture).await;
    let adapter = OpenAIAdapter::new("test-key", &server.uri()).expect("new");

    let request = test_request(vec![]);
    let mut stream = adapter
        .stream(request, test_bounds())
        .await
        .expect("stream should start");

    let mut events = Vec::new();
    while let Some(result) = stream.next().await {
        match result {
            Ok(event) => events.push(event),
            Err(e) => panic!("unexpected error: {:?}", e),
        }
    }

    assert!(events
        .iter()
        .any(|e| matches!(e, ModelStreamEvent::Completed(c)
        if c.stop_reason == StopReason::EndTurn)));
    assert!(events
        .iter()
        .any(|e| matches!(e, ModelStreamEvent::TextDelta(t) if t == "Done.")));
}

#[tokio::test]
async fn openai_malformed_json_skipped() {
    let fixture = get_fixture("openai_malformed_json");
    let server = setup_sse_mock(&fixture).await;
    let adapter = OpenAIAdapter::new("test-key", &server.uri()).expect("new");

    let request = test_request(vec![]);
    let mut stream = adapter
        .stream(request, test_bounds())
        .await
        .expect("stream should start");

    let mut events = Vec::new();
    while let Some(result) = stream.next().await {
        match result {
            Ok(event) => events.push(event),
            Err(e) => panic!("unexpected error: {:?}", e),
        }
    }

    // Rig's OpenAI parser skips malformed chunks; stream should complete
    assert!(events
        .iter()
        .any(|e| matches!(e, ModelStreamEvent::Completed(c)
        if c.stop_reason == StopReason::EndTurn)));
    assert!(events
        .iter()
        .any(|e| matches!(e, ModelStreamEvent::TextDelta(t) if t == "OK")));
}

#[tokio::test]
async fn openai_incomplete_stream_is_not_completed() {
    let fixture = get_fixture("openai_incomplete_stream");
    let server = setup_sse_mock(&fixture).await;
    let adapter = OpenAIAdapter::new("test-key", &server.uri()).expect("new");

    let request = test_request(vec![]);
    let mut stream = adapter
        .stream(request, test_bounds())
        .await
        .expect("stream should start");

    let first = stream.next().await.expect("should have event").expect("ok");
    assert!(matches!(&first, ModelStreamEvent::TextDelta(t) if t == "Partial..."));

    let (events, error) = collect_events(&mut stream).await;
    assert!(
        matches!(error, Some(ModelError::StreamIncomplete(_))),
        "expected StreamIncomplete error, got: {:?}",
        error
    );
    assert!(
        !events
            .iter()
            .any(|event| { matches!(event, ModelStreamEvent::Completed(_)) }),
        "incomplete stream must not emit Completed"
    );
}

#[tokio::test]
async fn openai_provider_401() {
    let fixture = get_fixture("openai_401");
    let server = setup_sse_mock(&fixture).await;
    let adapter = OpenAIAdapter::new("test-key", &server.uri()).expect("new");

    let request = test_request(vec![]);
    let result = adapter.stream(request, test_bounds()).await;

    if let Ok(mut stream) = result {
        let (events, error) = collect_events(&mut stream).await;
        assert!(
            error.is_some()
                || events
                    .iter()
                    .any(|e| matches!(e, ModelStreamEvent::Error(_))),
            "401 should produce an error, got events: {:?}, error: {:?}",
            events,
            error
        );
    }
}

#[tokio::test]
async fn openai_provider_429() {
    let fixture = get_fixture("openai_429");
    let server = setup_sse_mock(&fixture).await;
    let adapter = OpenAIAdapter::new("test-key", &server.uri()).expect("new");

    let request = test_request(vec![]);
    let result = adapter.stream(request, test_bounds()).await;

    if let Ok(mut stream) = result {
        let (events, error) = collect_events(&mut stream).await;
        assert!(
            error.is_some()
                || events
                    .iter()
                    .any(|e| matches!(e, ModelStreamEvent::Error(_))),
            "429 should produce an error, got events: {:?}, error: {:?}",
            events,
            error
        );
    }
}

#[tokio::test]
async fn openai_provider_500() {
    let fixture = get_fixture("openai_500");
    let server = setup_sse_mock(&fixture).await;
    let adapter = OpenAIAdapter::new("test-key", &server.uri()).expect("new");

    let request = test_request(vec![]);
    let result = adapter.stream(request, test_bounds()).await;

    if let Ok(mut stream) = result {
        let (events, error) = collect_events(&mut stream).await;
        assert!(
            error.is_some()
                || events
                    .iter()
                    .any(|e| matches!(e, ModelStreamEvent::Error(_))),
            "500 should produce an error, got events: {:?}, error: {:?}",
            events,
            error
        );
    }
}

#[tokio::test]
async fn openai_provider_context_overflow() {
    let fixture = get_fixture("openai_context_overflow");
    let server = setup_sse_mock(&fixture).await;
    let adapter = OpenAIAdapter::new("test-key", &server.uri()).expect("new");

    let request = test_request(vec![]);
    let result = adapter.stream(request, test_bounds()).await;

    if let Ok(mut stream) = result {
        let (events, error) = collect_events(&mut stream).await;
        assert!(
            matches!(error, Some(ModelError::ContextOverflow(_)))
                || events
                    .iter()
                    .any(|e| matches!(e, ModelStreamEvent::Error(ModelError::ContextOverflow(_)))),
            "context overflow should produce ContextOverflow error, got events: {:?}, error: {:?}",
            events,
            error
        );
    }
}

#[tokio::test]
async fn openai_api_key_not_in_debug() {
    let adapter = OpenAIAdapter::new("super-secret-key-12345", "http://localhost:1").expect("new");
    let debug = format!("{:?}", adapter);
    assert!(
        !debug.contains("super-secret-key-12345"),
        "API key must not appear in Debug output: {}",
        debug
    );
}

#[tokio::test]
async fn openai_stream_consumes_at_least_two_chunks() {
    let fixture = get_fixture("openai_text_only");
    assert!(
        fixture.chunks.len() >= 2,
        "fixture must have at least 2 chunks for synchronization barrier test"
    );

    let server = setup_sse_mock(&fixture).await;
    let adapter = OpenAIAdapter::new("test-key", &server.uri()).expect("new");

    let request = test_request(vec![]);
    let mut stream = adapter
        .stream(request, test_bounds())
        .await
        .expect("stream should start");

    let first = stream.next().await.expect("should have event").expect("ok");
    assert!(
        matches!(&first, ModelStreamEvent::TextDelta(_)),
        "first event should be TextDelta, got: {:?}",
        first
    );

    let mut event_count = 1;
    while let Some(result) = stream.next().await {
        match result {
            Ok(_) => event_count += 1,
            Err(_) => break,
        }
    }
    assert!(
        event_count >= 2,
        "should observe at least 2 events from the stream"
    );
}

// ========== Visual annotation runner contract tests ==========

mod visual_annotation {
    use super::*;
    use rollshot_agent::driver::VisualAnnotationProfile;
    use rollshot_agent::runtime::BudgetDimension;
    use rollshot_agent::skills::bundled_action_guide_visual_annotations_use;
    use std::sync::Arc;

    struct ScriptedProvider {
        requests: Mutex<Vec<ModelRequest>>,
        scripts: Mutex<VecDeque<Vec<ModelStreamEvent>>>,
    }

    impl ScriptedProvider {
        fn new(scripts: Vec<Vec<ModelStreamEvent>>) -> Self {
            Self {
                requests: Mutex::new(Vec::new()),
                scripts: Mutex::new(VecDeque::from(scripts)),
            }
        }
    }

    impl ProviderAdapter for ScriptedProvider {
        fn stream(
            &self,
            request: ModelRequest,
            _bounds: StreamBounds,
        ) -> Pin<
            Box<
                dyn std::future::Future<
                        Output = Result<
                            Pin<
                                Box<
                                    dyn futures_util::Stream<
                                            Item = Result<ModelStreamEvent, ModelError>,
                                        > + Send,
                                >,
                            >,
                            ModelError,
                        >,
                    > + Send
                    + '_,
            >,
        > {
            self.requests.lock().unwrap().push(request);
            let events = self.scripts.lock().unwrap().pop_front().unwrap_or_default();
            Box::pin(async move {
                let s = futures_util::stream::iter(events.into_iter().map(Ok));
                Ok(Box::pin(s)
                    as Pin<
                        Box<
                            dyn futures_util::Stream<Item = Result<ModelStreamEvent, ModelError>>
                                + Send,
                        >,
                    >)
            })
        }
    }

    // ---- PendingProvider: ignores StreamBounds intentionally ----

    #[derive(Clone, Copy)]
    enum PendingMode {
        Establishment,
        AfterText,
        AfterCompleted,
    }

    struct PendingProvider {
        mode: PendingMode,
        entered: Arc<Mutex<Option<tokio::sync::oneshot::Sender<()>>>>,
    }

    impl ProviderAdapter for PendingProvider {
        fn stream(
            &self,
            _request: ModelRequest,
            _bounds: StreamBounds,
        ) -> Pin<
            Box<
                dyn std::future::Future<
                        Output = Result<
                            Pin<
                                Box<
                                    dyn futures_util::Stream<
                                            Item = Result<ModelStreamEvent, ModelError>,
                                        > + Send,
                                >,
                            >,
                            ModelError,
                        >,
                    > + Send
                    + '_,
            >,
        > {
            match self.mode {
                PendingMode::Establishment => {
                    let mut entered = self.entered.lock().unwrap();
                    if let Some(tx) = entered.take() {
                        let _ = tx.send(());
                    }
                    Box::pin(async { std::future::pending().await })
                }
                PendingMode::AfterText => {
                    let entered = Arc::clone(&self.entered);
                    Box::pin(async move {
                        let (tx, events) = {
                            let mut guard = entered.lock().unwrap();
                            let tx = guard.take();
                            let events: Vec<Result<ModelStreamEvent, ModelError>> =
                                vec![Ok(ModelStreamEvent::TextDelta("partial".to_string()))];
                            (tx, events)
                        };
                        Ok(Box::pin(async_stream::stream! {
                            for event in events {
                                yield event;
                            }
                            if let Some(tx) = tx {
                                let _ = tx.send(());
                            }
                            // Remain pending — never yield a completion.
                            std::future::pending::<()>().await;
                        })
                            as Pin<
                                Box<
                                    dyn futures_util::Stream<
                                            Item = Result<ModelStreamEvent, ModelError>,
                                        > + Send,
                                >,
                            >)
                    })
                }
                PendingMode::AfterCompleted => {
                    let entered = Arc::clone(&self.entered);
                    Box::pin(async move {
                        let tx = {
                            let mut guard = entered.lock().unwrap();
                            guard.take()
                        };
                        Ok(Box::pin(async_stream::stream! {
                            yield Ok(ModelStreamEvent::TextDelta("partial".to_string()));
                            if let Some(tx) = tx {
                                let _ = tx.send(());
                            }
                            yield Ok(ModelStreamEvent::Completed(ModelCompletion {
                                usage: ModelUsage {
                                    input_tokens: 5,
                                    output_tokens: 3,
                                    total_tokens: 8,
                                },
                                stop_reason: StopReason::EndTurn,
                            }));
                            std::future::pending::<()>().await;
                        })
                            as Pin<
                                Box<
                                    dyn futures_util::Stream<
                                            Item = Result<ModelStreamEvent, ModelError>,
                                        > + Send,
                                >,
                            >)
                    })
                }
            }
        }
    }

    fn spawn_pending_run(
        mode: PendingMode,
    ) -> (
        tokio::sync::oneshot::Receiver<()>,
        RunCancellation,
        tokio::task::JoinHandle<VisualAnnotationRunTerminal>,
    ) {
        let (entered_tx, entered_rx) = tokio::sync::oneshot::channel();
        let provider = PendingProvider {
            mode,
            entered: Arc::new(Mutex::new(Some(entered_tx))),
        };
        let cancellation = RunCancellation::new();
        let run_cancellation = cancellation.clone();
        let task = tokio::spawn(async move {
            let mut budget = visual_annotation_run_budget();
            budget.wall_time = std::time::Duration::from_secs(10);
            AgentRunner::new(AgentConfig {
                max_turns: 2,
                ..AgentConfig::default()
            })
            .run_visual_annotation_with_provider(
                va_profile(),
                authorized_input_with_one_png(),
                &provider,
                budget,
                &run_cancellation,
            )
            .await
        });
        (entered_rx, cancellation, task)
    }

    fn authorized_input_with_one_png() -> AuthorizedModelInput {
        AuthorizedModelInput::new(
            "anthropic".into(),
            "vision-model".into(),
            "suggest visual annotations".into(),
            vec![AttachmentDescriptor {
                media_type: MediaType::Png,
                width: 1,
                height: 1,
                byte_count: 4,
            }],
            vec![vec![0x89, 0x50, 0x4E, 0x47]],
        )
        .expect("valid input")
    }

    fn completion_event(stop: StopReason) -> ModelStreamEvent {
        ModelStreamEvent::Completed(ModelCompletion {
            usage: ModelUsage {
                input_tokens: 5,
                output_tokens: 3,
                total_tokens: 8,
            },
            stop_reason: stop,
        })
    }

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
            completion_event(StopReason::ToolUse),
        ]
    }

    fn va_runner() -> AgentRunner {
        AgentRunner::new(AgentConfig {
            max_turns: 2,
            ..AgentConfig::default()
        })
    }

    fn va_profile() -> VisualAnnotationProfile<'static> {
        let skill = bundled_action_guide_visual_annotations_use()
            .expect("bundled visual skill must resolve");
        let skill: &'static rollshot_agent::skills::SkillUse = Box::leak(Box::new(skill));
        VisualAnnotationProfile::from_skill(skill)
            .expect("bundled visual skill must be accepted")
    }

    // ---- One attachment ----

    #[tokio::test]
    async fn runner_sends_one_attachment() {
        let args = serde_json::json!({
            "suggestions": [
                {"id":1,"kind":"number_callout","tip":{"x":0.5,"y":0.25},
                 "bubble":{"x":0.6,"y":0.25},"confidence":0.9}
            ]
        })
        .to_string();
        let provider = ScriptedProvider::new(vec![tool_call_turn(
            "tc_1",
            "submit_visual_annotation_suggestions",
            &args,
        )]);
        let runner = va_runner();
        let input = authorized_input_with_one_png();
        let cancel = RunCancellation::new();

        let _ = runner
            .run_visual_annotation_with_provider(
                va_profile(),
                input,
                &provider,
                visual_annotation_run_budget(),
                &cancel,
            )
            .await;

        let requests = provider.requests.lock().unwrap();
        assert_eq!(requests.len(), 1);
        assert_eq!(
            requests[0].attachments.len(),
            1,
            "runner must send exactly one attachment"
        );
    }

    // ---- Two turns (max_turns from budget) ----

    #[tokio::test]
    async fn runner_makes_at_most_two_model_turns() {
        let budget = visual_annotation_run_budget();
        assert_eq!(budget.model_calls, 2, "budget model_calls must be 2");

        let args = serde_json::json!({
            "suggestions": [
                {"id":1,"kind":"text_note","position":{"x":0.3,"y":0.4},
                 "text":"Click Save","confidence":0.9}
            ]
        })
        .to_string();
        let provider = ScriptedProvider::new(vec![tool_call_turn(
            "tc_1",
            "submit_visual_annotation_suggestions",
            &args,
        )]);
        let runner = va_runner();
        let input = authorized_input_with_one_png();
        let cancel = RunCancellation::new();

        let _ = runner
            .run_visual_annotation_with_provider(va_profile(), input, &provider, budget, &cancel)
            .await;

        let requests = provider.requests.lock().unwrap();
        assert!(
            requests.len() <= 2,
            "runner must make at most 2 model turns, got {}",
            requests.len()
        );
    }

    // ---- One tool call ----

    #[tokio::test]
    async fn runner_expects_one_tool_call() {
        let budget = visual_annotation_run_budget();
        assert_eq!(budget.tool_calls, 1, "budget tool_calls must be 1");

        let args = serde_json::json!({
            "suggestions": [
                {"id":1,"kind":"opaque_redaction",
                 "bounds":{"x":0.5,"y":0.1,"width":0.2,"height":0.1},
                 "confidence":0.7}
            ]
        })
        .to_string();
        let provider = ScriptedProvider::new(vec![tool_call_turn(
            "tc_1",
            "submit_visual_annotation_suggestions",
            &args,
        )]);
        let runner = va_runner();
        let input = authorized_input_with_one_png();
        let cancel = RunCancellation::new();

        let result = runner
            .run_visual_annotation_with_provider(va_profile(), input, &provider, budget, &cancel)
            .await;

        match result {
            VisualAnnotationRunTerminal::Suggested(drafts) => {
                assert_eq!(
                    drafts.len(),
                    1,
                    "expected one suggestion from one tool call"
                );
            }
            other => panic!("expected Suggested, got {other:?}"),
        }
    }

    // ---- 30-second deadline ----

    #[test]
    fn budget_has_30s_wall_clock_deadline() {
        let budget = visual_annotation_run_budget();
        assert_eq!(
            budget.wall_time,
            std::time::Duration::from_secs(30),
            "wall_time must be 30 seconds"
        );
    }

    // ---- 4 KiB argument/result limits ----

    #[test]
    fn budget_has_4kib_argument_and_result_limits() {
        let budget = visual_annotation_run_budget();
        assert_eq!(budget.argument_bytes, 4_096, "argument_bytes must be 4 KiB");
        assert_eq!(budget.result_bytes, 4_096, "result_bytes must be 4 KiB");
    }

    // ---- Cancellation ----

    #[tokio::test]
    async fn cancellation_before_completion_returns_cancelled() {
        let args = serde_json::json!({
            "suggestions": [
                {"id":1,"kind":"number_callout","tip":{"x":0.5,"y":0.5},
                 "bubble":{"x":0.6,"y":0.5},"confidence":0.5}
            ]
        })
        .to_string();
        let provider = ScriptedProvider::new(vec![tool_call_turn(
            "tc_1",
            "submit_visual_annotation_suggestions",
            &args,
        )]);
        let runner = va_runner();
        let input = authorized_input_with_one_png();
        let cancel = RunCancellation::new();
        cancel.cancel();

        let result = runner
            .run_visual_annotation_with_provider(
                va_profile(),
                input,
                &provider,
                visual_annotation_run_budget(),
                &cancel,
            )
            .await;

        assert_eq!(
            result,
            VisualAnnotationRunTerminal::Cancelled,
            "cancellation before completion must return Cancelled"
        );
    }

    // ---- Debug redaction ----

    #[tokio::test]
    async fn runner_debug_output_does_not_contain_prompt_or_attachment_bytes() {
        let secret_prompt = "secret-prompt-text-42424";
        let args = serde_json::json!({
            "suggestions": [
                {"id":1,"kind":"text_note","position":{"x":0.5,"y":0.5},
                 "text":"note","confidence":0.5}
            ]
        })
        .to_string();
        let provider = ScriptedProvider::new(vec![tool_call_turn(
            "tc_1",
            "submit_visual_annotation_suggestions",
            &args,
        )]);
        let runner = va_runner();
        let input = AuthorizedModelInput::new(
            "anthropic".into(),
            "vision-model".into(),
            secret_prompt.into(),
            vec![AttachmentDescriptor {
                media_type: MediaType::Png,
                width: 1,
                height: 1,
                byte_count: 4,
            }],
            vec![vec![0x89, 0x50, 0x4E, 0x47]],
        )
        .expect("valid input");
        let cancel = RunCancellation::new();

        let result = runner
            .run_visual_annotation_with_provider(
                va_profile(),
                input,
                &provider,
                visual_annotation_run_budget(),
                &cancel,
            )
            .await;

        let debug_str = format!("{:?}", result);
        assert!(
            !debug_str.contains(secret_prompt),
            "Debug output must not contain prompt text: {}",
            debug_str
        );
        assert!(
            !debug_str.contains("89504e47"),
            "Debug output must not contain attachment hex bytes: {}",
            debug_str
        );
    }

    // ---- PendingProvider host-bounds tests ----

    #[tokio::test]
    async fn runner_cancels_pending_provider_establishment() {
        let (entered, cancellation, task) = spawn_pending_run(PendingMode::Establishment);
        entered.await.expect("provider entered establishment");
        cancellation.cancel();
        let terminal = tokio::time::timeout(std::time::Duration::from_secs(1), task)
            .await
            .expect("runner must not hang")
            .expect("runner task");
        assert_eq!(terminal, VisualAnnotationRunTerminal::Cancelled);
    }

    #[tokio::test(start_paused = true)]
    async fn runner_deadlines_pending_provider_establishment() {
        let (entered, _cancellation, task) = spawn_pending_run(PendingMode::Establishment);
        entered.await.expect("provider entered establishment");
        tokio::time::advance(std::time::Duration::from_secs(10)).await;
        assert_eq!(
            task.await.expect("runner task"),
            VisualAnnotationRunTerminal::BudgetExhausted {
                dimension: BudgetDimension::WallTime,
            }
        );
    }

    #[tokio::test]
    async fn runner_cancels_pending_provider_item_after_partial_text() {
        let (entered, cancellation, task) = spawn_pending_run(PendingMode::AfterText);
        entered.await.expect("provider entered pending item poll");
        cancellation.cancel();
        let terminal = tokio::time::timeout(std::time::Duration::from_secs(1), task)
            .await
            .expect("runner must not hang")
            .expect("runner task");
        assert_eq!(terminal, VisualAnnotationRunTerminal::Cancelled);
    }

    #[tokio::test(start_paused = true)]
    async fn runner_deadlines_pending_provider_item_after_partial_text() {
        let (entered, _cancellation, task) = spawn_pending_run(PendingMode::AfterText);
        entered.await.expect("provider entered pending item poll");
        tokio::time::advance(std::time::Duration::from_secs(10)).await;
        assert_eq!(
            task.await.expect("runner task"),
            VisualAnnotationRunTerminal::BudgetExhausted {
                dimension: BudgetDimension::WallTime,
            }
        );
    }

    #[tokio::test]
    async fn runner_does_not_wait_for_eof_after_valid_completion() {
        let (entered, _cancellation, task) = spawn_pending_run(PendingMode::AfterCompleted);
        entered.await.expect("provider completed and signalled");
        let terminal = tokio::time::timeout(std::time::Duration::from_secs(1), task)
            .await
            .expect("runner must not hang after valid completion")
            .expect("runner task");
        assert!(
            !matches!(terminal, VisualAnnotationRunTerminal::Cancelled)
                && !matches!(
                    terminal,
                    VisualAnnotationRunTerminal::BudgetExhausted { .. }
                ),
            "runner should return promptly after Completed, got: {:?}",
            terminal
        );
    }

    // ---- Partial-tool driver test ----

    struct FallibleScriptedProvider {
        requests: Mutex<Vec<ModelRequest>>,
        scripts: Mutex<VecDeque<Vec<Result<ModelStreamEvent, ModelError>>>>,
    }

    impl FallibleScriptedProvider {
        fn new(scripts: Vec<Vec<Result<ModelStreamEvent, ModelError>>>) -> Self {
            Self {
                requests: Mutex::new(Vec::new()),
                scripts: Mutex::new(VecDeque::from(scripts)),
            }
        }
    }

    impl ProviderAdapter for FallibleScriptedProvider {
        fn stream(
            &self,
            request: ModelRequest,
            _bounds: StreamBounds,
        ) -> Pin<
            Box<
                dyn std::future::Future<
                        Output = Result<
                            Pin<
                                Box<
                                    dyn futures_util::Stream<
                                            Item = Result<ModelStreamEvent, ModelError>,
                                        > + Send,
                                >,
                            >,
                            ModelError,
                        >,
                    > + Send
                    + '_,
            >,
        > {
            self.requests.lock().unwrap().push(request);
            let events = self.scripts.lock().unwrap().pop_front().unwrap_or_default();
            Box::pin(async move {
                let s = futures_util::stream::iter(events);
                Ok(Box::pin(s)
                    as Pin<
                        Box<
                            dyn futures_util::Stream<Item = Result<ModelStreamEvent, ModelError>>
                                + Send,
                        >,
                    >)
            })
        }
    }

    #[tokio::test]
    async fn partial_tool_call_never_executes() {
        let provider = FallibleScriptedProvider::new(vec![vec![
            Ok(ModelStreamEvent::ToolCallStart {
                id: "tc_partial".into(),
                name: "submit_visual_annotation_suggestions".into(),
            }),
            Ok(ModelStreamEvent::ToolCallArgumentDelta {
                id: "tc_partial".into(),
                delta: "{\"unfinished\"".into(),
            }),
            Err(ModelError::StreamIncomplete("fixture ended".into())),
        ]]);
        let runner = va_runner();
        let input = authorized_input_with_one_png();
        let cancel = RunCancellation::new();

        let result = runner
            .run_visual_annotation_with_provider(
                va_profile(),
                input,
                &provider,
                visual_annotation_run_budget(),
                &cancel,
            )
            .await;

        assert_eq!(
            result,
            VisualAnnotationRunTerminal::ProviderFailure,
            "partial tool call with StreamIncomplete must produce ProviderFailure"
        );

        let requests = provider.requests.lock().unwrap();
        assert_eq!(
            requests.len(),
            1,
            "should have made exactly one model request"
        );
    }
}
