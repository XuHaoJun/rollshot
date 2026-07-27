#[cfg(all(feature = "rig-039", feature = "rig-040"))]
compile_error!("enable exactly one of rig-039 or rig-040");
#[cfg(not(any(feature = "rig-039", feature = "rig-040")))]
compile_error!("enable exactly one of rig-039 or rig-040");

#[cfg(feature = "rig-039")]
use rig_core_039 as rig;
#[cfg(feature = "rig-040")]
use rig_core_040 as rig;

use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
struct Fixture {
    chunks: Vec<String>,
}

#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum Observation {
    Text { text: String },
    ToolCall { id: String, name: String },
    Final { total_tokens: u64 },
    Error { category: String },
    End,
}

fn fixture(name: &str) -> Fixture {
    let all: serde_json::Value = serde_json::from_str(include_str!(
        "../fixtures/cases.json"
    ))
    .expect("fixture JSON must parse");
    serde_json::from_value(all[name].clone()).expect("named fixture must parse")
}

fn request() -> rig::completion::CompletionRequest {
    rig::completion::CompletionRequest {
        model: Some("test-model".to_string()),
        preamble: None,
        chat_history: rig::OneOrMany::one(rig::message::Message::user("probe")),
        documents: vec![],
        tools: vec![],
        temperature: None,
        max_tokens: Some(64),
        tool_choice: None,
        additional_params: None,
        output_schema: None,
    }
}

async fn sse_server(case: &Fixture) -> wiremock::MockServer {
    use wiremock::{Mock, MockServer, ResponseTemplate};
    let server = MockServer::start().await;
    let response = ResponseTemplate::new(200)
        .set_body_bytes(case.chunks.join("").into_bytes())
        .insert_header("content-type", "text/event-stream");
    Mock::given(wiremock::matchers::any())
        .respond_with(response)
        .mount(&server)
        .await;
    server
}

async fn probe_anthropic(name: &str) -> Vec<Observation> {
    use futures_util::StreamExt;
    use rig::client::CompletionClient;
    use rig::completion::{CompletionModel, GetTokenUsage};
    use rig::streaming::StreamedAssistantContent;

    let case = fixture(name);
    let server = sse_server(&case).await;
    let client = rig::providers::anthropic::Client::builder()
        .api_key("spike-key")
        .base_url(&server.uri())
        .build()
        .expect("anthropic client");
    let model = client.completion_model("test-model");
    let mut stream = model.stream(request()).await.expect("stream establishment");
    let mut out = Vec::new();

    while let Some(item) = stream.next().await {
        match item {
            Ok(StreamedAssistantContent::Text(text)) => {
                out.push(Observation::Text { text: text.text });
            }
            Ok(StreamedAssistantContent::ToolCall { tool_call, .. }) => {
                out.push(Observation::ToolCall {
                    id: tool_call.id,
                    name: tool_call.function.name,
                });
            }
            Ok(StreamedAssistantContent::Final(response)) => {
                out.push(Observation::Final {
                    total_tokens: response.token_usage().total_tokens,
                });
            }
            Ok(_) => {}
            Err(error) => {
                out.push(Observation::Error {
                    category: format!("{error:?}"),
                });
                break;
            }
        }
    }
    out.push(Observation::End);
    out
}

async fn probe_openai(name: &str) -> Vec<Observation> {
    use futures_util::StreamExt;
    use rig::client::CompletionClient;
    use rig::completion::{CompletionModel, GetTokenUsage};
    use rig::streaming::StreamedAssistantContent;

    let case = fixture(name);
    let server = sse_server(&case).await;
    let client = rig::providers::openai::Client::builder()
        .api_key("spike-key")
        .base_url(&server.uri())
        .build()
        .expect("openai client")
        .completions_api();
    let model = client.completion_model("test-model");
    let mut stream = model.stream(request()).await.expect("stream establishment");
    let mut out = Vec::new();

    while let Some(item) = stream.next().await {
        match item {
            Ok(StreamedAssistantContent::Text(text)) => {
                out.push(Observation::Text { text: text.text });
            }
            Ok(StreamedAssistantContent::ToolCall { tool_call, .. }) => {
                out.push(Observation::ToolCall {
                    id: tool_call.id,
                    name: tool_call.function.name,
                });
            }
            Ok(StreamedAssistantContent::Final(response)) => {
                out.push(Observation::Final {
                    total_tokens: response.token_usage().total_tokens,
                });
            }
            Ok(_) => {}
            Err(error) => {
                out.push(Observation::Error {
                    category: format!("{error:?}"),
                });
                break;
            }
        }
    }
    out.push(Observation::End);
    out
}

#[tokio::main]
async fn main() {
    let mut args = std::env::args().skip(1);
    let provider = args.next().expect("provider argument");
    let fixture_name = args.next().expect("fixture argument");
    assert!(args.next().is_none(), "only provider and fixture are accepted");
    let observations = match provider.as_str() {
        "anthropic" => probe_anthropic(&fixture_name).await,
        "openai" => probe_openai(&fixture_name).await,
        other => panic!("unsupported provider: {other}"),
    };
    println!(
        "{}",
        serde_json::to_string_pretty(&observations).expect("serialize observations")
    );
}
