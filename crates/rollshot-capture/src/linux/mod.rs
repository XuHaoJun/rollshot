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
        let mut session = self.portal.start(options.clone())?;
        let (fd, node_id) = session.take_resources();
        let stream = LinuxPortalFrameStream::new(fd, node_id, options)?;
        Ok(Box::new(stream))
    }
}

#[cfg(test)]
mod tests {
    use super::LinuxPortalBackend;
    use crate::backend::CaptureBackend;
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

    struct EnvGuard {
        key: &'static str,
        original: Option<String>,
    }

    impl EnvGuard {
        fn set(key: &'static str, value: &str) -> Self {
            let original = std::env::var(key).ok();
            std::env::set_var(key, value);
            Self { key, original }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            match &self.original {
                Some(val) => std::env::set_var(self.key, val),
                None => std::env::remove_var(self.key),
            }
        }
    }

    #[test]
    fn start_succeeds_in_test_build() {
        let mut backend = LinuxPortalBackend::new();
        let _guard = EnvGuard::set("XDG_SESSION_TYPE", "wayland");
        let result = backend.start(CaptureOptions::default());
        assert!(result.is_ok(), "expected Ok from start");
    }
}
