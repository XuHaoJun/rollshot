use std::fmt;

// ---------- Opaque IDs ----------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SessionId(u64);

impl SessionId {
    pub fn new(raw: u64) -> Self {
        Self(raw)
    }

    pub fn get(self) -> u64 {
        self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct RunId(u64);

impl RunId {
    pub fn new(raw: u64) -> Self {
        Self(raw)
    }

    pub fn get(self) -> u64 {
        self.0
    }
}

// ---------- Media types ----------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MediaType {
    Png,
    Jpeg,
}

impl MediaType {
    pub fn from_mime(mime: &str) -> Option<Self> {
        match mime {
            "image/png" => Some(Self::Png),
            "image/jpeg" => Some(Self::Jpeg),
            _ => None,
        }
    }
}

// ---------- Attachment descriptors ----------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AttachmentDescriptor {
    pub media_type: MediaType,
    pub width: u32,
    pub height: u32,
    pub byte_count: u64,
}

// ---------- Limits ----------

pub const MAX_ATTACHMENTS: usize = 8;
pub const MAX_BYTES_PER_ATTACHMENT: u64 = 10 * 1024 * 1024; // 10 MiB
pub const MAX_TOTAL_BYTES: u64 = 40 * 1024 * 1024; // 40 MiB

// ---------- Manifest ----------

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthorizedInputManifest {
    pub provider: String,
    pub model: String,
    pub descriptors: Vec<AttachmentDescriptor>,
}

impl AuthorizedInputManifest {
    pub fn total_bytes(&self) -> Option<u64> {
        self.descriptors
            .iter()
            .try_fold(0u64, |acc, d| acc.checked_add(d.byte_count))
    }
}

// ---------- Errors ----------

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum InputError {
    #[error("descriptor count does not match attachment count")]
    DescriptorMismatch,
    #[error("unsupported media type")]
    UnsupportedMediaType,
    #[error("too many attachments: got {got}, max {max}")]
    AttachmentCountOverflow { got: usize, max: usize },
    #[error("declared byte count {declared} does not match actual payload length {actual}")]
    ByteCountMismatch { declared: u64, actual: u64 },
    #[error("attachment dimensions must be non-zero: {width}x{height}")]
    InvalidDimensions { width: u32, height: u32 },
    #[error("attachment too large: {bytes} bytes exceeds {max} byte limit")]
    PerAttachmentOverflow { bytes: u64, max: u64 },
    #[error("total attachment bytes {bytes} exceeds {max} byte limit")]
    TotalByteOverflow { bytes: u64, max: u64 },
}

// ---------- Authorized model input ----------

const ATTACHMENT_REDACTED: &str = "<redacted-attachment>";
const USER_TEXT_REDACTED: &str = "<redacted-user-text>";

pub struct AuthorizedModelInput {
    pub manifest: AuthorizedInputManifest,
    pub user_message: String,
    attachments: Vec<Vec<u8>>,
}

impl AuthorizedModelInput {
    pub fn new(
        provider: String,
        model: String,
        user_message: String,
        descriptors: Vec<AttachmentDescriptor>,
        attachment_bytes: Vec<Vec<u8>>,
    ) -> Result<Self, InputError> {
        if descriptors.len() != attachment_bytes.len() {
            return Err(InputError::DescriptorMismatch);
        }
        if descriptors.len() > MAX_ATTACHMENTS {
            return Err(InputError::AttachmentCountOverflow {
                got: descriptors.len(),
                max: MAX_ATTACHMENTS,
            });
        }
        for (desc, payload) in descriptors.iter().zip(attachment_bytes.iter()) {
            if desc.width == 0 || desc.height == 0 {
                return Err(InputError::InvalidDimensions {
                    width: desc.width,
                    height: desc.height,
                });
            }
            let actual =
                u64::try_from(payload.len()).map_err(|_| InputError::PerAttachmentOverflow {
                    bytes: desc.byte_count,
                    max: MAX_BYTES_PER_ATTACHMENT,
                })?;
            if actual != desc.byte_count {
                return Err(InputError::ByteCountMismatch {
                    declared: desc.byte_count,
                    actual,
                });
            }
        }
        for desc in &descriptors {
            if desc.byte_count > MAX_BYTES_PER_ATTACHMENT {
                return Err(InputError::PerAttachmentOverflow {
                    bytes: desc.byte_count,
                    max: MAX_BYTES_PER_ATTACHMENT,
                });
            }
        }
        let manifest = AuthorizedInputManifest {
            provider,
            model,
            descriptors,
        };
        let total = manifest.total_bytes().unwrap_or(u64::MAX);
        if total > MAX_TOTAL_BYTES {
            return Err(InputError::TotalByteOverflow {
                bytes: total,
                max: MAX_TOTAL_BYTES,
            });
        }
        Ok(Self {
            manifest,
            user_message,
            attachments: attachment_bytes,
        })
    }

    pub fn attachments(&self) -> &[Vec<u8>] {
        &self.attachments
    }

    #[allow(dead_code)] // Used by the callout runner in a later task
    pub(crate) fn take_model_attachments(&mut self) -> Vec<crate::model::ModelAttachment> {
        self.manifest
            .descriptors
            .iter()
            .zip(std::mem::take(&mut self.attachments))
            .map(|(descriptor, bytes)| {
                crate::model::ModelAttachment::new(
                    descriptor.media_type,
                    descriptor.width,
                    descriptor.height,
                    std::sync::Arc::from(bytes),
                )
            })
            .collect()
    }
}

impl fmt::Debug for AuthorizedModelInput {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AuthorizedModelInput")
            .field("manifest", &self.manifest)
            .field("user_message", &USER_TEXT_REDACTED)
            .field("attachments", &ATTACHMENT_REDACTED)
            .finish()
    }
}

// ---------- Agent session ----------

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Turn {
    pub role: Role,
    pub text: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    User,
    Assistant,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompletedExchange {
    pub user: Turn,
    pub assistant: Turn,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SessionError {
    #[error("cannot append: no completed turn pair yet")]
    IncompleteTurn,
}

#[derive(Debug, Clone)]
pub struct AgentSession {
    pub session_id: SessionId,
    exchanges: Vec<CompletedExchange>,
    pending_user: Option<String>,
}

impl AgentSession {
    pub fn new(session_id: SessionId) -> Self {
        Self {
            session_id,
            exchanges: Vec::new(),
            pending_user: None,
        }
    }

    pub fn push_user(&mut self, text: String) {
        self.pending_user = Some(text);
    }

    pub fn push_assistant(&mut self, text: String) -> Result<(), SessionError> {
        let user_text = self
            .pending_user
            .take()
            .ok_or(SessionError::IncompleteTurn)?;
        self.exchanges.push(CompletedExchange {
            user: Turn {
                role: Role::User,
                text: user_text,
            },
            assistant: Turn {
                role: Role::Assistant,
                text,
            },
        });
        Ok(())
    }

    pub fn exchanges(&self) -> &[CompletedExchange] {
        &self.exchanges
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_ids_with_different_values_are_not_equal() {
        let a = SessionId::new(1);
        let b = SessionId::new(2);
        assert_ne!(a, b);
    }

    #[test]
    fn run_ids_with_different_values_are_not_equal() {
        let a = RunId::new(10);
        let b = RunId::new(20);
        assert_ne!(a, b);
    }

    #[test]
    fn session_id_preserves_inner_value() {
        let id = SessionId::new(42);
        assert_eq!(id.get(), 42);
    }

    #[test]
    fn run_id_preserves_inner_value() {
        let id = RunId::new(99);
        assert_eq!(id.get(), 99);
    }

    #[test]
    fn manifest_total_bytes_sums_descriptors() {
        let manifest = AuthorizedInputManifest {
            provider: "openai".into(),
            model: "gpt-4o".into(),
            descriptors: vec![
                AttachmentDescriptor {
                    media_type: MediaType::Png,
                    width: 100,
                    height: 100,
                    byte_count: 1000,
                },
                AttachmentDescriptor {
                    media_type: MediaType::Jpeg,
                    width: 200,
                    height: 200,
                    byte_count: 2000,
                },
            ],
        };
        assert_eq!(manifest.total_bytes(), Some(3000));
    }

    #[test]
    fn manifest_provider_and_model_are_stored() {
        let manifest = AuthorizedInputManifest {
            provider: "anthropic".into(),
            model: "claude-3".into(),
            descriptors: vec![],
        };
        assert_eq!(manifest.provider, "anthropic");
        assert_eq!(manifest.model, "claude-3");
    }

    #[test]
    fn reject_descriptor_count_mismatch() {
        let result = AuthorizedModelInput::new(
            "openai".into(),
            "gpt-4o".into(),
            "hello".into(),
            vec![AttachmentDescriptor {
                media_type: MediaType::Png,
                width: 10,
                height: 10,
                byte_count: 100,
            }],
            vec![],
        );
        assert_eq!(result.unwrap_err(), InputError::DescriptorMismatch);
    }

    #[test]
    fn reject_unsupported_media_type() {
        assert!(MediaType::from_mime("image/webp").is_none());
        assert!(MediaType::from_mime("image/png").is_some());
        assert!(MediaType::from_mime("image/jpeg").is_some());
    }

    #[test]
    fn reject_too_many_attachments() {
        let descriptors: Vec<_> = (0..=MAX_ATTACHMENTS)
            .map(|_| AttachmentDescriptor {
                media_type: MediaType::Png,
                width: 1,
                height: 1,
                byte_count: 1,
            })
            .collect();
        let bytes: Vec<_> = descriptors.iter().map(|_| vec![0u8]).collect();
        let result = AuthorizedModelInput::new(
            "openai".into(),
            "gpt-4o".into(),
            "hello".into(),
            descriptors,
            bytes,
        );
        match result.unwrap_err() {
            InputError::AttachmentCountOverflow { got, max } => {
                assert_eq!(got, MAX_ATTACHMENTS + 1);
                assert_eq!(max, MAX_ATTACHMENTS);
            }
            other => panic!("expected AttachmentCountOverflow, got {other:?}"),
        }
    }

    #[test]
    fn reject_per_attachment_byte_overflow() {
        let oversize = MAX_BYTES_PER_ATTACHMENT + 1;
        let result = AuthorizedModelInput::new(
            "openai".into(),
            "gpt-4o".into(),
            "hello".into(),
            vec![AttachmentDescriptor {
                media_type: MediaType::Png,
                width: 1,
                height: 1,
                byte_count: oversize,
            }],
            vec![vec![0u8; oversize as usize]],
        );
        match result.unwrap_err() {
            InputError::PerAttachmentOverflow { bytes, max } => {
                assert_eq!(bytes, oversize);
                assert_eq!(max, MAX_BYTES_PER_ATTACHMENT);
            }
            other => panic!("expected PerAttachmentOverflow, got {other:?}"),
        }
    }

    #[test]
    fn reject_total_byte_overflow() {
        // Each attachment is under per-attachment limit but total exceeds.
        let big = MAX_BYTES_PER_ATTACHMENT - 1; // just under per-attachment max
        let result = AuthorizedModelInput::new(
            "openai".into(),
            "gpt-4o".into(),
            "hello".into(),
            vec![
                AttachmentDescriptor {
                    media_type: MediaType::Png,
                    width: 1,
                    height: 1,
                    byte_count: big,
                },
                AttachmentDescriptor {
                    media_type: MediaType::Png,
                    width: 1,
                    height: 1,
                    byte_count: big,
                },
                AttachmentDescriptor {
                    media_type: MediaType::Jpeg,
                    width: 1,
                    height: 1,
                    byte_count: big,
                },
                AttachmentDescriptor {
                    media_type: MediaType::Jpeg,
                    width: 1,
                    height: 1,
                    byte_count: big,
                },
                AttachmentDescriptor {
                    media_type: MediaType::Png,
                    width: 1,
                    height: 1,
                    byte_count: big,
                },
            ],
            vec![vec![0u8; big as usize]; 5],
        );
        match result.unwrap_err() {
            InputError::TotalByteOverflow { bytes, max } => {
                assert!(bytes > max);
                assert_eq!(max, MAX_TOTAL_BYTES);
            }
            other => panic!("expected TotalByteOverflow, got {other:?}"),
        }
    }

    #[test]
    fn valid_input_construction_succeeds() {
        let input = AuthorizedModelInput::new(
            "openai".into(),
            "gpt-4o".into(),
            "describe this".into(),
            vec![AttachmentDescriptor {
                media_type: MediaType::Png,
                width: 640,
                height: 480,
                byte_count: 1024,
            }],
            vec![vec![0xAB; 1024]],
        )
        .expect("valid input should succeed");
        assert_eq!(input.manifest.provider, "openai");
        assert_eq!(input.manifest.model, "gpt-4o");
        assert_eq!(input.attachments().len(), 1);
    }

    #[test]
    fn debug_output_contains_descriptors_but_not_attachment_bytes() {
        let input = AuthorizedModelInput::new(
            "openai".into(),
            "gpt-4o".into(),
            "test".into(),
            vec![AttachmentDescriptor {
                media_type: MediaType::Png,
                width: 10,
                height: 10,
                byte_count: 50,
            }],
            vec![vec![0xDE; 50]],
        )
        .unwrap();
        let dbg = format!("{input:?}");
        assert!(dbg.contains("manifest"), "Debug should include manifest");
        assert!(
            !dbg.contains("deadbeef") && !dbg.contains("DEADBEEF") && !dbg.contains("\\xde\\xad"),
            "Debug must not leak raw attachment bytes"
        );
        assert!(
            dbg.contains("<redacted-attachment>"),
            "Debug should contain redaction sentinel"
        );
    }

    #[test]
    fn debug_output_redacts_user_message() {
        let input = AuthorizedModelInput::new(
            "openai".into(),
            "gpt-4o".into(),
            "hello world".into(),
            vec![],
            vec![],
        )
        .unwrap();
        let dbg = format!("{input:?}");
        assert!(
            !dbg.contains("hello world"),
            "Debug must not leak user text"
        );
        assert!(
            dbg.contains("<redacted-user-text>"),
            "Debug should contain user text redaction sentinel"
        );
    }

    #[test]
    fn session_stores_completed_exchanges() {
        let mut session = AgentSession::new(SessionId::new(1));
        session.push_user("what is 2+2?".into());
        session.push_assistant("4".into()).unwrap();
        assert_eq!(session.exchanges().len(), 1);
        assert_eq!(session.exchanges()[0].user.text, "what is 2+2?");
        assert_eq!(session.exchanges()[0].assistant.text, "4");
    }

    #[test]
    fn session_rejects_assistant_without_pending_user() {
        let mut session = AgentSession::new(SessionId::new(1));
        let result = session.push_assistant("hello".into());
        assert_eq!(result.unwrap_err(), SessionError::IncompleteTurn);
    }

    #[test]
    fn session_multiple_exchanges_in_order() {
        let mut session = AgentSession::new(SessionId::new(1));
        session.push_user("first".into());
        session.push_assistant("reply-1".into()).unwrap();
        session.push_user("second".into());
        session.push_assistant("reply-2".into()).unwrap();
        assert_eq!(session.exchanges().len(), 2);
        assert_eq!(session.exchanges()[0].user.text, "first");
        assert_eq!(session.exchanges()[1].assistant.text, "reply-2");
    }

    #[test]
    fn session_debug_shows_exchanges() {
        let mut session = AgentSession::new(SessionId::new(1));
        session.push_user("q".into());
        session.push_assistant("a".into()).unwrap();
        let dbg = format!("{session:?}");
        assert!(dbg.contains("q"));
        assert!(dbg.contains("a"));
    }

    #[test]
    fn authorized_input_builds_model_attachments_without_revalidation() {
        let mut input = AuthorizedModelInput::new(
            "anthropic".into(),
            "vision-model".into(),
            "inspect".into(),
            vec![AttachmentDescriptor {
                media_type: MediaType::Png,
                width: 2,
                height: 3,
                byte_count: 4,
            }],
            vec![vec![1, 2, 3, 4]],
        )
        .unwrap();

        let attachments = input.take_model_attachments();
        assert_eq!(attachments.len(), 1);
        assert_eq!(attachments[0].media_type(), MediaType::Png);
        assert_eq!(attachments[0].bytes(), &[1, 2, 3, 4]);
        assert!(input.attachments().is_empty());
    }

    #[test]
    fn rejects_declared_byte_count_that_does_not_match_payload() {
        let error = AuthorizedModelInput::new(
            "anthropic".into(),
            "m".into(),
            "p".into(),
            vec![AttachmentDescriptor {
                media_type: MediaType::Png,
                width: 1,
                height: 1,
                byte_count: 1,
            }],
            vec![vec![1, 2]],
        )
        .unwrap_err();
        assert_eq!(
            error,
            InputError::ByteCountMismatch {
                declared: 1,
                actual: 2
            }
        );
    }

    #[test]
    fn rejects_zero_sized_attachment_dimensions() {
        let error = AuthorizedModelInput::new(
            "anthropic".into(),
            "m".into(),
            "p".into(),
            vec![AttachmentDescriptor {
                media_type: MediaType::Png,
                width: 0,
                height: 1,
                byte_count: 1,
            }],
            vec![vec![1]],
        )
        .unwrap_err();
        assert_eq!(
            error,
            InputError::InvalidDimensions {
                width: 0,
                height: 1
            }
        );
    }
}
