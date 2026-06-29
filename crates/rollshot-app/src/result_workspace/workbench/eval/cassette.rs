use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use wiremock::{Request, Respond, ResponseTemplate};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct CassetteFile {
    pub version: u32,
    pub metadata: CassetteMeta,
    #[serde(default)]
    pub attachment: Option<AttachmentMeta>,
    pub interactions: Vec<Interaction>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct CassetteMeta {
    pub recorded_at: String,
    pub provider: String,
    pub model: String,
    pub substitutions: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct AttachmentMeta {
    pub media_type: String,
    pub width: u32,
    pub height: u32,
    pub byte_count: u64,
    pub sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct Interaction {
    pub request: RecordedRequest,
    pub response: RecordedResponse,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct RecordedRequest {
    pub method: String,
    pub url_path: String,
    pub headers: BTreeMap<String, String>,
    pub body_summary: RecordedRequestBody,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum RecordedRequestBody {
    JsonWithImage {
        base64: String,
        byte_count: u64,
        sha256: String,
    },
    JsonWithoutImage {
        byte_count: u64,
        sha256: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct RecordedResponse {
    pub status: u16,
    pub headers: BTreeMap<String, String>,
    pub sse_body: String,
}

/// Replays a cassette's interactions in recorded order, one per request.
pub(crate) struct CassetteResponder {
    interactions: Vec<Interaction>,
    cursor: AtomicUsize,
}

impl CassetteResponder {
    pub fn new(interactions: Vec<Interaction>) -> Self {
        Self {
            interactions,
            cursor: AtomicUsize::new(0),
        }
    }

    pub(crate) fn next_interaction(&self) -> Interaction {
        let i = self.cursor.fetch_add(1, Ordering::SeqCst);
        self.interactions.get(i).cloned().unwrap_or_else(|| {
            panic!("cassette exhausted: model call {i} has no recorded interaction")
        })
    }
}

impl Respond for CassetteResponder {
    fn respond(&self, _req: &Request) -> ResponseTemplate {
        let interaction = self.next_interaction();
        ResponseTemplate::new(interaction.response.status)
            .insert_header("content-type", "text/event-stream")
            .set_body_bytes(interaction.response.sse_body.into_bytes())
    }
}

pub(crate) fn sha256_hex(bytes: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(bytes);
    h.finalize().iter().map(|b| format!("{b:02x}")).collect()
}

pub(crate) fn load_cassette(intent: &str) -> CassetteFile {
    let path = super::fixture::fixtures_root()
        .join(intent)
        .join("cassette.json");
    let data =
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    serde_json::from_str(&data).expect("valid cassette.json")
}

/// Strips sensitive headers and replaces image base64 with attachment metadata.
/// Mutates in place so callers can serialize the redacted result.
pub(crate) fn redact_cassette(cassette: &mut CassetteFile) {
    let sensitive_headers = ["authorization", "x-api-key"];
    for interaction in &mut cassette.interactions {
        for key in &sensitive_headers {
            interaction.request.headers.remove(*key);
        }
        if let RecordedRequestBody::JsonWithImage {
            byte_count, sha256, ..
        } = &interaction.request.body_summary
        {
            interaction.request.body_summary = RecordedRequestBody::JsonWithoutImage {
                byte_count: *byte_count,
                sha256: sha256.clone(),
            };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn interaction(body: &str) -> Interaction {
        Interaction {
            request: RecordedRequest {
                method: "POST".into(),
                url_path: "/v1/messages".into(),
                headers: BTreeMap::new(),
                body_summary: RecordedRequestBody::JsonWithoutImage {
                    byte_count: 0,
                    sha256: sha256_hex(b""),
                },
            },
            response: RecordedResponse {
                status: 200,
                headers: BTreeMap::new(),
                sse_body: body.into(),
            },
        }
    }

    #[test]
    fn responder_returns_interactions_in_order() {
        let responder = CassetteResponder::new(vec![interaction("a"), interaction("b")]);
        assert_eq!(responder.next_interaction().response.sse_body, "a");
        assert_eq!(responder.next_interaction().response.sse_body, "b");
    }

    #[test]
    #[should_panic(expected = "cassette exhausted")]
    fn responder_panics_clearly_when_exhausted() {
        let responder = CassetteResponder::new(vec![interaction("a")]);
        let _ = responder.next_interaction();
        let _ = responder.next_interaction();
    }

    #[test]
    fn sha256_is_stable() {
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn redaction_strips_auth_headers_and_image_base64() {
        let image_bytes = b"fake-png-data";
        let image_sha = sha256_hex(image_bytes);
        let image_base64 = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJ";

        let mut headers = BTreeMap::new();
        headers.insert("authorization".into(), "Bearer sk-secret-key-12345".into());
        headers.insert("x-api-key".into(), "xk-secret-key-67890".into());
        headers.insert("content-type".into(), "application/json".into());

        let mut cassette = CassetteFile {
            version: 1,
            metadata: CassetteMeta {
                recorded_at: "2026-01-01T00:00:00Z".into(),
                provider: "anthropic".into(),
                model: "claude-opus-4-8".into(),
                substitutions: "none".into(),
            },
            attachment: Some(AttachmentMeta {
                media_type: "image/png".into(),
                width: 800,
                height: 600,
                byte_count: image_bytes.len() as u64,
                sha256: image_sha.clone(),
            }),
            interactions: vec![Interaction {
                request: RecordedRequest {
                    method: "POST".into(),
                    url_path: "/v1/messages".into(),
                    headers,
                    body_summary: RecordedRequestBody::JsonWithImage {
                        base64: image_base64.into(),
                        byte_count: 1024,
                        sha256: sha256_hex(b"request-body"),
                    },
                },
                response: RecordedResponse {
                    status: 200,
                    headers: BTreeMap::new(),
                    sse_body: "data: {\"type\":\"message_start\"}\n\n".into(),
                },
            }],
        };

        let request_sha = sha256_hex(b"request-body");

        redact_cassette(&mut cassette);
        let json = serde_json::to_string(&cassette).unwrap();

        assert!(
            !json.contains("sk-secret-key-12345"),
            "authorization header survived serialization"
        );
        assert!(
            !json.contains("xk-secret-key-67890"),
            "x-api-key header survived serialization"
        );
        assert!(
            !json.contains(image_base64),
            "image base64 survived redaction"
        );
        assert!(
            !json.contains("json_with_image"),
            "body kind should be json_without_image after redaction"
        );
        assert!(
            json.contains(&image_sha),
            "image sha256 must be present in attachment metadata"
        );
        assert!(
            json.contains(&request_sha),
            "request body sha256 must be preserved"
        );
        assert!(
            json.contains("content-type"),
            "non-sensitive headers should be preserved"
        );
    }
}
