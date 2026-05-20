use crate::error::CaptureError;
use crate::types::{CaptureOptions, CaptureProbe, CapturedFrame};

pub trait CaptureBackend {
    fn name(&self) -> &'static str;
    fn probe(&self) -> CaptureProbe;
    fn start(
        &mut self,
        options: CaptureOptions,
    ) -> Result<Box<dyn FrameStream>, CaptureError>;
}

pub trait FrameStream: Send {
    fn next_frame(&mut self) -> Result<CapturedFrame, CaptureError>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackendKind {
    Fixture,
    LinuxPortalPipeWire,
    MacosScreenCaptureKit,
    Unsupported,
}

impl BackendKind {
    pub fn as_flag(self) -> &'static str {
        match self {
            BackendKind::Fixture => "fixture",
            BackendKind::LinuxPortalPipeWire => "linux-portal",
            BackendKind::MacosScreenCaptureKit => "macos-sck",
            BackendKind::Unsupported => "unsupported",
        }
    }

    pub fn from_cli_flag(flag: &str) -> Result<Self, CaptureError> {
        match flag {
            "auto" => Ok(default_backend()),
            "fixture" => Ok(BackendKind::Fixture),
            "linux-portal" => Ok(BackendKind::LinuxPortalPipeWire),
            "macos-sck" => Ok(BackendKind::MacosScreenCaptureKit),
            other => Err(CaptureError::InvalidConfig {
                message: format!(
                    "unknown backend '{other}'; expected one of: auto, fixture, linux-portal, macos-sck"
                ),
            }),
        }
    }
}

pub fn default_backend() -> BackendKind {
    let session_type = std::env::var("XDG_SESSION_TYPE").ok();
    default_backend_for(std::env::consts::OS, session_type.as_deref())
}

/// Pure helper for `default_backend` — exposed for tests so they can exercise
/// the OS / session decision matrix without mutating process-global env vars.
pub fn default_backend_for(os: &str, session_type: Option<&str>) -> BackendKind {
    match os {
        "macos" => BackendKind::MacosScreenCaptureKit,
        "linux" => match session_type {
            Some("wayland") => BackendKind::LinuxPortalPipeWire,
            _ => BackendKind::Unsupported,
        },
        _ => BackendKind::Unsupported,
    }
}

#[cfg(test)]
mod tests {
    use super::{default_backend_for, BackendKind};
    use crate::error::CaptureError;

    #[test]
    fn from_cli_flag_accepts_known_backends() {
        assert_eq!(
            BackendKind::from_cli_flag("fixture").unwrap(),
            BackendKind::Fixture
        );
        assert_eq!(
            BackendKind::from_cli_flag("linux-portal").unwrap(),
            BackendKind::LinuxPortalPipeWire
        );
        assert_eq!(
            BackendKind::from_cli_flag("macos-sck").unwrap(),
            BackendKind::MacosScreenCaptureKit
        );
        BackendKind::from_cli_flag("auto").expect("auto resolves");
    }

    #[test]
    fn from_cli_flag_rejects_unknown() {
        let err = BackendKind::from_cli_flag("nope").expect_err("unknown rejected");
        match err {
            CaptureError::InvalidConfig { message } => {
                assert!(message.contains("nope"), "msg = {message}");
                assert!(message.contains("fixture"), "msg = {message}");
            }
            other => panic!("expected InvalidConfig, got {other:?}"),
        }
    }

    #[test]
    fn as_flag_round_trips() {
        for kind in [
            BackendKind::Fixture,
            BackendKind::LinuxPortalPipeWire,
            BackendKind::MacosScreenCaptureKit,
        ] {
            assert_eq!(
                BackendKind::from_cli_flag(kind.as_flag()).unwrap(),
                kind
            );
        }
    }

    #[test]
    fn default_backend_for_decision_matrix() {
        assert_eq!(
            default_backend_for("macos", None),
            BackendKind::MacosScreenCaptureKit
        );
        assert_eq!(
            default_backend_for("macos", Some("wayland")),
            BackendKind::MacosScreenCaptureKit
        );
        assert_eq!(
            default_backend_for("linux", Some("wayland")),
            BackendKind::LinuxPortalPipeWire
        );
        assert_eq!(
            default_backend_for("linux", Some("tty")),
            BackendKind::Unsupported
        );
        assert_eq!(default_backend_for("linux", None), BackendKind::Unsupported);
        assert_eq!(default_backend_for("windows", None), BackendKind::Unsupported);
    }
}
