//! Build- and storage-time errors for the vision host.
//!
//! Capability-call-time failures use `rollshot_automation::CapabilityError`;
//! this type is only for construction and template-store operations that
//! happen outside the capability call chain.

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum VisionError {
    #[error("image is empty (zero width or height)")]
    EmptyImage,
    #[error("template bytes invalid: {code}")]
    InvalidTemplateBytes { code: &'static str },
    #[error("candidate bounds are outside the source image")]
    CandidateOutOfBounds,
    #[error("template store limit exceeded: {code}")]
    StoreLimit { code: &'static str },
    #[error("io/serialization failure: {code}")]
    Io { code: &'static str },
}
