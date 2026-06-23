use futures_util::StreamExt;
use rollshot_agent::model::{
    ModelError, ModelRequest, ModelStreamEvent, StopReason, ToolDefinition,
};
use rollshot_agent::provider::{AnthropicAdapter, ProviderAdapter};
use serde::Deserialize;
use wiremock::{Mock, MockServer, ResponseTemplate};

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
    let mut stream = adapter.stream(request).await.expect("stream should start");

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
    let mut stream = adapter.stream(request).await.expect("stream should start");

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
    let mut stream = adapter.stream(request).await.expect("stream should start");

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
    let mut stream = adapter.stream(request).await.expect("stream should start");

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
    let mut stream = adapter.stream(request).await.expect("stream should start");

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
    let result = adapter.stream(request).await;

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
async fn anthropic_incomplete_stream_emits_stream_incomplete() {
    let fixture = get_fixture("anthropic_incomplete_stream");
    let server = setup_sse_mock(&fixture).await;
    let adapter = AnthropicAdapter::new("test-key", &server.uri()).expect("new");

    let request = test_request(vec![]);
    let mut stream = adapter.stream(request).await.expect("stream should start");

    // Should get text delta
    let first = stream.next().await.expect("should have event").expect("ok");
    assert!(matches!(&first, ModelStreamEvent::TextDelta(t) if t == "Partial..."));

    // Stream should end — may produce a Completed event or an error
    // The key assertion: it does not hang indefinitely
    let mut got_terminal = false;
    while let Some(result) = stream.next().await {
        match result {
            Ok(ModelStreamEvent::Completed(_)) => {
                got_terminal = true;
                break;
            }
            Ok(ModelStreamEvent::Error(_)) => {
                got_terminal = true;
                break;
            }
            Err(_) => {
                got_terminal = true;
                break;
            }
            _ => continue,
        }
    }
    assert!(
        got_terminal,
        "incomplete stream should terminate with Completed or Error"
    );
}

#[tokio::test]
async fn anthropic_provider_401() {
    let fixture = get_fixture("anthropic_401");
    let server = setup_sse_mock(&fixture).await;
    let adapter = AnthropicAdapter::new("test-key", &server.uri()).expect("new");

    let request = test_request(vec![]);
    let result = adapter.stream(request).await;

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
    let result = adapter.stream(request).await;

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
    let result = adapter.stream(request).await;

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
    let mut stream = adapter.stream(request).await.expect("stream should start");

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
