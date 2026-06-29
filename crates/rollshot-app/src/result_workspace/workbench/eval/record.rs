use std::collections::BTreeMap;
use std::sync::Arc;

use image::GenericImageView;
use rollshot_agent::domain::{AttachmentDescriptor, AuthorizedModelInput, MediaType, SessionId};
use rollshot_agent::driver::{AgentConfig, AgentRunner, RunTerminalState};
use rollshot_agent::runtime::{RunBudget, RunCancellation, RunEvent, RunEventSink};
use rollshot_agent::tools::ToolContext;
use rollshot_agent::AnthropicAdapter;
use tokio::sync::Mutex;

use super::cassette::{
    redact_cassette, sha256_hex, AttachmentMeta, CassetteFile, CassetteMeta, Interaction,
    RecordedRequest, RecordedRequestBody, RecordedResponse,
};
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

type TurnCapture = (
    BTreeMap<String, String>,
    Vec<u8>,
    u16,
    BTreeMap<String, String>,
    Vec<u8>,
);

struct TeeProxy {
    turns: Arc<Mutex<Vec<TurnCapture>>>,
    upstream: String,
}

impl TeeProxy {
    fn new(upstream: String) -> Self {
        Self {
            turns: Arc::new(Mutex::new(Vec::new())),
            upstream,
        }
    }

    async fn handle(
        turns: Arc<Mutex<Vec<TurnCapture>>>,
        upstream: String,
        mut req: hyper::Request<hyper::body::Incoming>,
    ) -> Result<hyper::Response<http_body_util::Full<bytes::Bytes>>, hyper::Error> {
        use http_body_util::BodyExt;

        let req_body = match req.body_mut().collect().await {
            Ok(b) => b.to_bytes(),
            Err(e) => {
                tracing::error!(target: "rollshot::eval::record", "read request body: {e}");
                return Ok(hyper::Response::builder()
                    .status(400)
                    .body(http_body_util::Full::new(bytes::Bytes::new()))
                    .unwrap());
            }
        };

        let mut headers = BTreeMap::new();
        for (k, v) in req.headers() {
            if let Ok(v) = v.to_str() {
                headers.insert(k.as_str().to_lowercase(), v.to_string());
            }
        }

        let url = format!(
            "{}{}",
            upstream,
            req.uri()
                .path_and_query()
                .map(|pq| pq.as_str())
                .unwrap_or("/")
        );

        let client = reqwest::Client::new();
        let mut rb = client
            .request(req.method().clone(), &url)
            .header("content-type", "application/json");

        for (k, v) in req.headers() {
            let kl = k.as_str().to_ascii_lowercase();
            if kl == "host" || kl == "content-type" || kl == "content-length" {
                continue;
            }
            rb = rb.header(k.clone(), v.clone());
        }
        rb = rb.body(req_body.clone());

        match rb.send().await {
            Ok(resp) => {
                let status = resp.status().as_u16();
                let mut resp_headers = BTreeMap::new();
                for (k, v) in resp.headers() {
                    if let Ok(v) = v.to_str() {
                        resp_headers.insert(k.as_str().to_lowercase(), v.to_string());
                    }
                }
                let body = match resp.bytes().await {
                    Ok(b) => b,
                    Err(e) => {
                        tracing::error!(target: "rollshot::eval::record", "read response body: {e}");
                        bytes::Bytes::new()
                    }
                };

                turns.lock().await.push((
                    headers,
                    req_body.to_vec(),
                    status,
                    resp_headers.clone(),
                    body.to_vec(),
                ));

                let mut resp_builder = hyper::Response::builder().status(status);
                for (k, v) in &resp_headers {
                    resp_builder = resp_builder.header(k.as_str(), v.as_str());
                }
                Ok(resp_builder
                    .body(http_body_util::Full::new(body))
                    .unwrap_or_else(|_| {
                        hyper::Response::builder()
                            .status(500)
                            .body(http_body_util::Full::new(bytes::Bytes::new()))
                            .unwrap()
                    }))
            }
            Err(e) => {
                tracing::error!(target: "rollshot::eval::record", "proxy request: {e}");
                Ok(hyper::Response::builder()
                    .status(502)
                    .body(http_body_util::Full::new(bytes::Bytes::new()))
                    .unwrap())
            }
        }
    }

    async fn start(self) -> (String, tokio::task::JoinHandle<()>) {
        let turns = self.turns.clone();
        let upstream = self.upstream.clone();
        let addr = std::net::SocketAddr::from(([127, 0, 0, 1], 0));

        let listener = tokio::net::TcpListener::bind(addr)
            .await
            .expect("bind proxy");
        let port = listener.local_addr().unwrap().port();
        let url = format!("http://127.0.0.1:{port}");

        let handle = tokio::spawn(async move {
            loop {
                let (stream, _) = match listener.accept().await {
                    Ok(s) => s,
                    Err(e) => {
                        tracing::error!(target: "rollshot::eval::record", "accept: {e}");
                        break;
                    }
                };
                let turns = turns.clone();
                let upstream = upstream.clone();
                tokio::spawn(async move {
                    let io = hyper_util::rt::tokio::TokioIo::new(stream);
                    let service = hyper::service::service_fn(move |req| {
                        let turns = turns.clone();
                        let upstream = upstream.clone();
                        async move { Self::handle(turns, upstream, req).await }
                    });
                    if let Err(e) = hyper_util::server::conn::auto::Builder::new(
                        hyper_util::rt::TokioExecutor::new(),
                    )
                    .serve_connection(io, service)
                    .await
                    {
                        tracing::error!(target: "rollshot::eval::record", "connection: {e}");
                    }
                });
            }
        });

        (url, handle)
    }
}

fn extract_image_meta(body: &[u8]) -> Option<AttachmentMeta> {
    let v: serde_json::Value = serde_json::from_slice(body).ok()?;
    let content = v.get("messages")?.as_array()?;
    for msg in content {
        let parts = msg.get("content")?.as_array()?;
        for part in parts {
            if part.get("type")?.as_str()? != "image" {
                continue;
            }
            let source = part.get("source")?;
            let b64 = source.get("data")?.as_str()?;
            let media = source.get("media_type")?.as_str()?;
            let decoded =
                base64::Engine::decode(&base64::engine::general_purpose::STANDARD, b64).ok()?;
            let sha = sha256_hex(&decoded);
            let dyn_img = image::load_from_memory(&decoded).ok()?;
            let (w, h) = dyn_img.dimensions();
            return Some(AttachmentMeta {
                media_type: media.to_string(),
                width: w,
                height: h,
                byte_count: decoded.len() as u64,
                sha256: sha,
            });
        }
    }
    None
}

fn build_interaction(
    req_headers: &BTreeMap<String, String>,
    req_body: &[u8],
    resp_status: u16,
    resp_headers: &BTreeMap<String, String>,
    resp_body: &[u8],
) -> Interaction {
    let body_summary = if let Some(meta) = extract_image_meta(req_body) {
        RecordedRequestBody::JsonWithImage {
            base64: String::new(),
            byte_count: meta.byte_count,
            sha256: meta.sha256,
        }
    } else {
        RecordedRequestBody::JsonWithoutImage {
            byte_count: req_body.len() as u64,
            sha256: sha256_hex(req_body),
        }
    };

    let mut hdrs = req_headers.clone();
    hdrs.remove("authorization");
    hdrs.remove("x-api-key");

    let mut resp_hdrs = resp_headers.clone();
    resp_hdrs.remove("set-cookie");

    let sse_body = String::from_utf8_lossy(resp_body).into_owned();

    Interaction {
        request: RecordedRequest {
            method: "POST".into(),
            url_path: "/v1/messages".into(),
            headers: hdrs,
            body_summary,
        },
        response: RecordedResponse {
            status: resp_status,
            headers: resp_hdrs,
            sse_body,
        },
    }
}

pub(crate) async fn record_cassette(
    intent: &str,
    real_base_url: &str,
    api_key: &str,
    model_override: Option<&str>,
) -> Result<(), String> {
    let image = super::fixture::load_image(intent);
    let meta = super::fixture::load_meta(intent);
    let (w, h) = image.dimensions();

    let proxy = TeeProxy::new(real_base_url.to_string());
    let turns_handle = proxy.turns.clone();
    let (proxy_url, _proxy_task) = proxy.start().await;

    let adapter =
        AnthropicAdapter::new(api_key, &proxy_url).map_err(|e| format!("adapter: {e:?}"))?;

    let vision = prepare_vision_context(&image, &ProductCapabilityBundle::empty())
        .map_err(|e| format!("prepare: {e:?}"))?;
    let cancellation = RunCancellation::new();
    let tool_ctx = Arc::new(ToolContext::new_with_capability_handles(
        SessionId::new(1),
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
    let host =
        vision.host.clone() as Arc<std::sync::Mutex<dyn rollshot_automation::AutomationHost>>;
    let executor: Arc<dyn rollshot_automation::AutomationExecutor> =
        Arc::new(rollshot_automation_rquickjs::QuickJsExecutor);
    let registry = build_authoring_tool_registry(tool_ctx.clone(), executor, host, inspection)
        .map_err(|e| format!("registry: {e:?}"))?;

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
    let model = model_override.unwrap_or(&meta.model);
    let input = AuthorizedModelInput::new(
        meta.provider.clone(),
        model.to_string(),
        format!("Redact the {} in this screenshot.", meta.intent),
        vec![descriptor],
        vec![png.clone()],
    )
    .map_err(|e| format!("input: {e:?}"))?;

    let runner = AgentRunner::new(AgentConfig::default());
    let mut session = rollshot_agent::domain::AgentSession::new(SessionId::new(1));
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

    match &terminal {
        RunTerminalState::ReadyForReview(_) => {}
        other => return Err(format!("non-terminal-ready: {other:?}")),
    }

    let image_meta = AttachmentMeta {
        media_type: "image/png".into(),
        width: w,
        height: h,
        byte_count: png.len() as u64,
        sha256: sha256_hex(&png),
    };

    let turns = turns_handle.lock().await;
    let interactions: Vec<Interaction> = turns
        .iter()
        .map(|(req_hdrs, req_body, resp_status, resp_hdrs, resp_body)| {
            build_interaction(req_hdrs, req_body, *resp_status, resp_hdrs, resp_body)
        })
        .collect();

    let now = chrono::Utc::now().to_rfc3339();
    let mut cassette = CassetteFile {
        version: 1,
        metadata: CassetteMeta {
            recorded_at: now,
            provider: meta.provider.clone(),
            model: model.to_string(),
            substitutions: "none".into(),
        },
        attachment: Some(image_meta),
        interactions,
    };
    redact_cassette(&mut cassette);

    let out_dir = super::fixture::fixtures_root().join(intent);
    std::fs::create_dir_all(&out_dir).map_err(|e| format!("mkdir: {e}"))?;
    let json = serde_json::to_string_pretty(&cassette).map_err(|e| format!("serialize: {e}"))?;
    std::fs::write(out_dir.join("cassette.json"), json).map_err(|e| format!("write: {e}"))?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    #[ignore]
    async fn record_one_fixture() {
        let _ = tracing_subscriber::fmt()
            .with_env_filter("rollshot=debug")
            .try_init();

        let intent = std::env::var("EVAL_INTENT").expect("EVAL_INTENT not set");
        if std::env::var("ROLLSHOT_RECORD_EVAL").is_err() {
            eprintln!("SKIP: ROLLSHOT_RECORD_EVAL not set");
            return;
        }

        let config_dir = dirs::config_dir().expect("no config dir").join("rollshot");
        let cfg =
            crate::result_workspace::workbench::provider_config::load_provider_config(&config_dir)
                .expect("load provider.toml");
        let api_key =
            crate::result_workspace::workbench::provider_config::resolve_key(&cfg.key_source)
                .expect("no API key resolved from provider.toml");
        let base_url = cfg
            .base_url
            .as_deref()
            .unwrap_or("https://api.anthropic.com");

        record_cassette(&intent, base_url, &api_key, Some(&cfg.model))
            .await
            .expect("record_cassette succeeded");
    }
}
