#![cfg(target_os = "linux")]

mod pipewire;
mod pixel;
mod portal;

use crate::backend::{CaptureBackend, FrameStream};
use crate::error::CaptureError;
use crate::types::{CaptureOptions, CaptureProbe};

use pipewire::LinuxPortalFrameStream;
use portal::PortalClient;

pub struct LinuxPortalBackend {
    portal: PortalClient,
}

impl LinuxPortalBackend {
    pub fn new() -> Self {
        Self {
            portal: PortalClient::new(),
        }
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
        self.portal.probe()
    }

    fn start(&mut self, options: CaptureOptions) -> Result<Box<dyn FrameStream>, CaptureError> {
        self.portal.start(options)?;
        Ok(Box::new(LinuxPortalFrameStream))
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
    fn start_succeeds_in_test_build() {
        let mut backend = LinuxPortalBackend::new();
        std::env::set_var("XDG_SESSION_TYPE", "wayland");
        let result = backend.start(CaptureOptions::default());
        assert!(result.is_ok(), "expected Ok from start");
    }
}
