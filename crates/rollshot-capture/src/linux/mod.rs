#![cfg(target_os = "linux")]

use crate::backend::{CaptureBackend, FrameStream};
use crate::error::CaptureError;
use crate::types::{CaptureOptions, CaptureProbe};

pub struct LinuxPortalBackend;

impl LinuxPortalBackend {
    pub fn new() -> Self {
        Self
    }
}

impl Default for LinuxPortalBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl CaptureBackend for LinuxPortalBackend {
    fn name(&self) -> &'static str {
        "linux-portal"
    }

    fn probe(&self) -> CaptureProbe {
        let session_type = std::env::var("XDG_SESSION_TYPE").unwrap_or_default();
        let desktop = std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default();

        let is_wayland = session_type == "wayland";
        let is_kde = desktop.to_ascii_lowercase().contains("kde")
            || desktop.to_ascii_lowercase().contains("plasma");

        let available = is_wayland && is_kde;
        let message = if available {
            "preconditions look ok; backend is not implemented in v0.1 plumbing phase".to_string()
        } else {
            "linux-portal requires a KDE/Plasma Wayland session".to_string()
        };

        CaptureProbe {
            backend: "linux-portal",
            available,
            message,
            details: vec![
                ("XDG_SESSION_TYPE".to_string(), session_type),
                ("XDG_CURRENT_DESKTOP".to_string(), desktop),
            ],
        }
    }

    fn start(
        &mut self,
        _options: CaptureOptions,
    ) -> Result<Box<dyn FrameStream>, CaptureError> {
        Err(CaptureError::NotImplemented {
            backend: "linux-portal",
        })
    }
}

#[cfg(test)]
mod tests {
    use super::LinuxPortalBackend;
    use crate::backend::CaptureBackend;
    use crate::error::CaptureError;
    use crate::types::CaptureOptions;

    #[test]
    fn probe_reports_env_in_details() {
        let backend = LinuxPortalBackend::new();
        let probe = backend.probe();
        assert_eq!(probe.backend, "linux-portal");
        let keys: Vec<&str> = probe.details.iter().map(|(k, _)| k.as_str()).collect();
        assert!(keys.contains(&"XDG_SESSION_TYPE"));
        assert!(keys.contains(&"XDG_CURRENT_DESKTOP"));
    }

    #[test]
    fn start_returns_not_implemented() {
        let mut backend = LinuxPortalBackend::new();
        match backend.start(CaptureOptions::default()) {
            Err(CaptureError::NotImplemented { backend }) => {
                assert_eq!(backend, "linux-portal");
            }
            Err(other) => panic!("expected NotImplemented, got {other:?}"),
            Ok(_) => panic!("expected error, got Ok"),
        }
    }
}
