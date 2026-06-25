use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum ProviderKind {
    #[default]
    Anthropic,
    OpenAI,
}

impl std::fmt::Display for ProviderKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Anthropic => write!(f, "Anthropic"),
            Self::OpenAI => write!(f, "OpenAI"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum KeySource {
    Env(String),
}

impl Default for KeySource {
    fn default() -> Self {
        Self::Env("ANTHROPIC_API_KEY".into())
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ProviderConfig {
    pub provider: ProviderKind,
    pub model: String,
    pub base_url: Option<String>,
    pub key_source: KeySource,
}
