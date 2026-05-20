#![cfg(target_os = "macos")]

use crate::backend::{CaptureBackend, FrameStream};
use crate::error::CaptureError;
use crate::types::{CaptureOptions, CaptureProbe};

pub struct MacosScreenCaptureKitBackend;

impl MacosScreenCaptureKitBackend {
    pub fn new() -> Self {
        Self
    }
}

impl Default for MacosScreenCaptureKitBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl CaptureBackend for MacosScreenCaptureKitBackend {
    fn name(&self) -> &'static str {
        "macos-sck"
    }

    fn probe(&self) -> CaptureProbe {
        CaptureProbe {
            backend: "macos-sck",
            available: true,
            message: "macOS host detected; backend is not implemented in v0.1 plumbing phase"
                .to_string(),
            details: vec![("os".to_string(), "macos".to_string())],
        }
    }

    fn start(&mut self, _options: CaptureOptions) -> Result<Box<dyn FrameStream>, CaptureError> {
        Err(CaptureError::NotImplemented {
            backend: "macos-sck",
        })
    }
}

#[cfg(test)]
mod tests {
    use super::MacosScreenCaptureKitBackend;
    use crate::backend::CaptureBackend;
    use crate::error::CaptureError;
    use crate::types::CaptureOptions;

    #[test]
    fn probe_reports_macos_in_details() {
        let backend = MacosScreenCaptureKitBackend::new();
        let probe = backend.probe();
        assert_eq!(probe.backend, "macos-sck");
        assert!(probe.details.iter().any(|(k, v)| k == "os" && v == "macos"));
    }

    #[test]
    fn start_returns_not_implemented() {
        let mut backend = MacosScreenCaptureKitBackend::new();
        match backend.start(CaptureOptions::default()) {
            Err(CaptureError::NotImplemented { backend }) => {
                assert_eq!(backend, "macos-sck");
            }
            Err(other) => panic!("expected NotImplemented, got {other:?}"),
            Ok(_) => panic!("expected error, got Ok"),
        }
    }
}
