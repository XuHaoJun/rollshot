use serde::Deserialize;
use std::fmt;
use std::path::{Path, PathBuf};
use std::str::FromStr;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Platform {
    Linux,
    Macos,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Modifier {
    Control,
    Alt,
    Shift,
    Command,
    Super,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Shortcut {
    modifiers: Vec<Modifier>,
    key: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DaemonConfig {
    pub capture_region_hotkey: Shortcut,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoadedConfig {
    pub config: DaemonConfig,
    pub warning: Option<String>,
}

#[derive(Deserialize)]
struct ConfigFile {
    daemon: RawDaemonConfig,
}

#[derive(Deserialize)]
struct RawDaemonConfig {
    capture_region_hotkey: String,
}

impl DaemonConfig {
    pub fn default_for(platform: Platform) -> Self {
        let text = match platform {
            Platform::Linux => "Alt+Shift+6",
            Platform::Macos => "Command+Shift+6",
        };
        Self {
            capture_region_hotkey: text.parse().expect("platform default is valid"),
        }
    }
}

impl FromStr for Shortcut {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        if value.trim().is_empty() || value.split('+').any(|part| part.trim().is_empty()) {
            return Err("shortcut contains an empty component".into());
        }
        let mut modifiers = Vec::new();
        let mut key = None;

        for part in value.split('+').map(str::trim) {
            let modifier = match part.to_ascii_lowercase().as_str() {
                "control" | "ctrl" => Some(Modifier::Control),
                "alt" | "option" => Some(Modifier::Alt),
                "shift" => Some(Modifier::Shift),
                "command" | "cmd" => Some(Modifier::Command),
                "super" | "logo" => Some(Modifier::Super),
                _ => None,
            };

            if let Some(modifier) = modifier {
                if modifiers.contains(&modifier) {
                    return Err(format!("duplicate modifier: {part}"));
                }
                modifiers.push(modifier);
            } else if key.replace(part.to_string()).is_some() {
                return Err("shortcut must contain exactly one base key".into());
            }
        }

        let key = key.ok_or_else(|| "shortcut must contain one base key".to_string())?;
        if modifiers.is_empty() {
            return Err("global shortcut must contain at least one modifier".into());
        }
        let function_key = key
            .strip_prefix('F')
            .and_then(|number| number.parse::<u8>().ok())
            .is_some_and(|number| (1..=24).contains(&number));
        if !(function_key || key.len() == 1 && key.chars().all(|ch| ch.is_ascii_alphanumeric())) {
            return Err("shortcut base key must be one ASCII letter/digit or F1-F24".into());
        }
        if modifiers.contains(&Modifier::Command) && modifiers.contains(&Modifier::Super) {
            return Err("Command and Super name the same platform modifier".into());
        }
        let key = if key.len() == 1 {
            key.to_ascii_lowercase()
        } else {
            key
        };
        Ok(Self { modifiers, key })
    }
}

impl Shortcut {
    pub fn portal_trigger(&self) -> String {
        let mut parts = Vec::new();
        for (modifier, name) in [
            (Modifier::Control, "CTRL"),
            (Modifier::Alt, "ALT"),
            (Modifier::Shift, "SHIFT"),
            (Modifier::Command, "LOGO"),
            (Modifier::Super, "LOGO"),
        ] {
            if self.modifiers.contains(&modifier) {
                parts.push(name);
            }
        }
        parts.push(&self.key);
        parts.join("+")
    }
}

impl fmt::Display for Shortcut {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut parts: Vec<&str> = self
            .modifiers
            .iter()
            .map(|modifier| match modifier {
                Modifier::Control => "Control",
                Modifier::Alt => "Alt",
                Modifier::Shift => "Shift",
                Modifier::Command => "Command",
                Modifier::Super => "Super",
            })
            .collect();
        parts.push(&self.key);
        write!(f, "{}", parts.join("+"))
    }
}

pub fn config_path() -> Result<PathBuf, String> {
    rollshot_config_dir().map(|dir| dir.join("config.toml"))
}

pub fn rollshot_config_dir() -> Result<PathBuf, String> {
    dirs::config_dir()
        .map(|dir| dir.join("rollshot"))
        .ok_or_else(|| "platform configuration directory is unavailable".to_string())
}

pub fn load_from(path: &Path, platform: Platform) -> LoadedConfig {
    let fallback = DaemonConfig::default_for(platform);
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return LoadedConfig {
                config: fallback,
                warning: None,
            };
        }
        Err(error) => {
            return LoadedConfig {
                config: fallback,
                warning: Some(format!("failed to read daemon config: {error}")),
            };
        }
    };

    let raw: ConfigFile = match toml::from_str(&text) {
        Ok(raw) => raw,
        Err(error) => {
            return LoadedConfig {
                config: fallback,
                warning: Some(format!("failed to parse daemon config: {error}")),
            };
        }
    };

    match raw.daemon.capture_region_hotkey.parse() {
        Ok(capture_region_hotkey) => LoadedConfig {
            config: DaemonConfig {
                capture_region_hotkey,
            },
            warning: None,
        },
        Err(error) => LoadedConfig {
            config: fallback,
            warning: Some(format!("invalid daemon shortcut: {error}")),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn absent_file_uses_linux_default_without_warning() {
        let dir = tempfile::tempdir().unwrap();
        let loaded = load_from(&dir.path().join("config.toml"), Platform::Linux);

        assert_eq!(
            loaded.config.capture_region_hotkey.to_string(),
            "Alt+Shift+6"
        );
        assert!(loaded.warning.is_none());
    }

    #[test]
    fn valid_file_overrides_default() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(
            &path,
            "[daemon]\ncapture_region_hotkey = \"Control+Alt+7\"\n",
        )
        .unwrap();

        let loaded = load_from(&path, Platform::Linux);

        assert_eq!(
            loaded.config.capture_region_hotkey.to_string(),
            "Control+Alt+7"
        );
        assert!(loaded.warning.is_none());
    }

    #[test]
    fn malformed_toml_falls_back_with_warning() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(&path, "[daemon\n").unwrap();

        let loaded = load_from(&path, Platform::Linux);

        assert_eq!(loaded.config, DaemonConfig::default_for(Platform::Linux));
        assert!(loaded.warning.unwrap().contains("parse"));
    }

    #[test]
    fn invalid_shortcut_falls_back_with_warning() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(&path, "[daemon]\ncapture_region_hotkey = \"Alt+Shift\"\n").unwrap();

        let loaded = load_from(&path, Platform::Linux);

        assert_eq!(loaded.config, DaemonConfig::default_for(Platform::Linux));
        assert!(loaded.warning.unwrap().contains("shortcut"));
    }

    #[test]
    fn empty_shortcut_component_falls_back_with_warning() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(&path, "[daemon]\ncapture_region_hotkey = \"Alt++6\"\n").unwrap();

        let loaded = load_from(&path, Platform::Linux);

        assert_eq!(loaded.config, DaemonConfig::default_for(Platform::Linux));
        assert!(loaded.warning.unwrap().contains("shortcut"));
    }

    #[test]
    fn bare_key_falls_back_instead_of_hijacking_normal_typing() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(&path, "[daemon]\ncapture_region_hotkey = \"6\"\n").unwrap();

        let loaded = load_from(&path, Platform::Linux);

        assert_eq!(loaded.config, DaemonConfig::default_for(Platform::Linux));
        assert!(loaded.warning.unwrap().contains("modifier"));
    }

    #[test]
    fn unreadable_path_falls_back_with_warning() {
        let dir = tempfile::tempdir().unwrap();
        let loaded = load_from(dir.path(), Platform::Linux);

        assert_eq!(loaded.config, DaemonConfig::default_for(Platform::Linux));
        assert!(loaded.warning.unwrap().contains("read"));
    }

    #[test]
    fn linux_portal_trigger_uses_xdg_modifier_names() {
        let shortcut: Shortcut = "Command+Control+Alt+Shift+6".parse().unwrap();
        assert_eq!(shortcut.portal_trigger(), "CTRL+ALT+SHIFT+LOGO+6");
    }

    #[test]
    fn duplicate_alias_and_out_of_range_function_keys_are_rejected() {
        for invalid in ["Alt+Alt+6", "Command+Super+6", "Alt+F25"] {
            assert!(invalid.parse::<Shortcut>().is_err(), "{invalid}");
        }
    }

    #[test]
    fn macos_default_keeps_command_first() {
        assert_eq!(
            DaemonConfig::default_for(Platform::Macos)
                .capture_region_hotkey
                .to_string(),
            "Command+Shift+6"
        );
    }
}
