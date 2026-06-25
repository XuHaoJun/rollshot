use std::path::{Path, PathBuf};

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

/// How the API key is resolved at runtime. Never persisted in the config file
/// (only the *name* of the env var is persisted; the value is read at runtime).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum KeySource {
    Env(String),
}

impl Default for KeySource {
    fn default() -> Self {
        Self::Env("ANTHROPIC_API_KEY".into())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderConfig {
    pub provider: ProviderKind,
    pub model: String,
    pub base_url: Option<String>,
    pub key_source: KeySource,
}

impl Default for ProviderConfig {
    fn default() -> Self {
        Self {
            provider: ProviderKind::Anthropic,
            model: "claude-sonnet-4-6".into(),
            base_url: None,
            key_source: KeySource::default(),
        }
    }
}

fn provider_config_path(config_dir: &Path) -> PathBuf {
    config_dir.join("provider.toml")
}

pub fn load_provider_config(config_dir: &Path) -> Result<ProviderConfig, String> {
    let path = provider_config_path(config_dir);
    match std::fs::read_to_string(&path) {
        Ok(text) => toml::from_str(&text).map_err(|_| "invalid provider.toml".to_string()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(ProviderConfig::default()),
        Err(e) => Err(format!("failed to read provider.toml: {e}")),
    }
}

pub fn save_provider_config(config_dir: &Path, cfg: &ProviderConfig) -> Result<(), String> {
    std::fs::create_dir_all(config_dir).map_err(|_| "create config dir".to_string())?;
    let path = provider_config_path(config_dir);
    let text = toml::to_string_pretty(cfg).map_err(|_| "serialize provider config".to_string())?;
    std::fs::write(&path, text).map_err(|_| "write provider.toml".to_string())
}

/// Resolve the API key from the given source. Returns None if unavailable.
pub fn resolve_key(source: &KeySource) -> Option<String> {
    match source {
        KeySource::Env(var) => std::env::var(var).ok().filter(|s| !s.is_empty()),
    }
}

pub fn has_key(cfg: &ProviderConfig) -> bool {
    resolve_key(&cfg.key_source).is_some()
}

pub fn provider_model_label(cfg: &ProviderConfig) -> String {
    format!("{} / {}", cfg.provider, cfg.model)
}

/// Build the provider adapter from the config. The adapter and the
/// `AuthorizedModelInput` are constructed from the same `ProviderConfig` so
/// `provider`/`model` strings match what the adapter streams (§10.7).
pub fn build_adapter(
    cfg: &ProviderConfig,
) -> Result<Box<dyn rollshot_agent::ProviderAdapter>, String> {
    let key = resolve_key(&cfg.key_source)
        .ok_or_else(|| "no provider key resolved".to_string())?;
    let base_url = cfg.base_url.as_deref().unwrap_or(match cfg.provider {
        ProviderKind::Anthropic => "https://api.anthropic.com",
        ProviderKind::OpenAI => "https://api.openai.com/v1",
    });
    Ok(match cfg.provider {
        ProviderKind::Anthropic => Box::new(
            rollshot_agent::AnthropicAdapter::new(&key, base_url)
                .map_err(|_| "anthropic adapter".to_string())?,
        ),
        ProviderKind::OpenAI => Box::new(
            rollshot_agent::OpenAIAdapter::new(&key, base_url)
                .map_err(|_| "openai adapter".to_string())?,
        ),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn default_provider_config() {
        let cfg = ProviderConfig::default();
        assert_eq!(cfg.provider, ProviderKind::Anthropic);
        assert_eq!(cfg.model, "claude-sonnet-4-6");
        assert!(cfg.base_url.is_none());
        assert!(matches!(cfg.key_source, KeySource::Env(ref v) if v == "ANTHROPIC_API_KEY"));
    }

    #[test]
    fn load_missing_file_returns_default() {
        let tmp = tempfile::tempdir().unwrap();
        let cfg = load_provider_config(tmp.path()).unwrap();
        assert_eq!(cfg.provider, ProviderKind::Anthropic);
    }

    #[test]
    fn load_round_trip() {
        let tmp = tempfile::tempdir().unwrap();
        let original = ProviderConfig {
            provider: ProviderKind::OpenAI,
            model: "gpt-4o".into(),
            base_url: Some("https://api.openai.com/v1".into()),
            key_source: KeySource::Env("OPENAI_API_KEY".into()),
        };
        save_provider_config(tmp.path(), &original).unwrap();
        let loaded = load_provider_config(tmp.path()).unwrap();
        assert_eq!(loaded.provider, ProviderKind::OpenAI);
        assert_eq!(loaded.model, "gpt-4o");
        assert_eq!(loaded.base_url.as_deref(), Some("https://api.openai.com/v1"));
    }

    #[test]
    fn resolve_env_key_absent_and_present() {
        let var = "TEST_ROLLSHOT_PROVIDER_KEY_928374";
        std::env::remove_var(var);
        assert_eq!(resolve_key(&KeySource::Env(var.into())), None);
        std::env::set_var(var, "sk-test");
        assert_eq!(
            resolve_key(&KeySource::Env(var.into())).as_deref(),
            Some("sk-test")
        );
        std::env::remove_var(var);
    }

    #[test]
    fn load_invalid_toml_returns_err() {
        let tmp = tempfile::tempdir().unwrap();
        fs::write(tmp.path().join("provider.toml"), "not = valid = toml").unwrap();
        assert!(load_provider_config(tmp.path()).is_err());
    }

    #[test]
    fn provider_model_label_format() {
        let cfg = ProviderConfig {
            provider: ProviderKind::Anthropic,
            model: "claude-sonnet-4-6".into(),
            base_url: None,
            key_source: KeySource::Env("X".into()),
        };
        assert_eq!(provider_model_label(&cfg), "Anthropic / claude-sonnet-4-6");
    }
}
