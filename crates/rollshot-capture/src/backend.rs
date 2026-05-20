use crate::error::CaptureError;
use crate::types::{CaptureOptions, CaptureProbe, CapturedFrame};

pub trait CaptureBackend {
    fn name(&self) -> &'static str;
    fn probe(&self) -> CaptureProbe;
    fn start(&mut self, options: CaptureOptions) -> Result<Box<dyn FrameStream>, CaptureError>;
}

pub trait FrameStream {
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

    pub fn create(self) -> Result<Box<dyn CaptureBackend>, CaptureError> {
        match self {
            BackendKind::Fixture => Err(CaptureError::InvalidConfig {
                message: "fixture backend requires --fixture <DIR>".to_string(),
            }),
            BackendKind::LinuxPortalPipeWire => {
                #[cfg(target_os = "linux")]
                {
                    Ok(Box::new(crate::linux::LinuxPortalBackend::new()))
                }
                #[cfg(not(target_os = "linux"))]
                {
                    Err(CaptureError::Unsupported {
                        message: "linux-portal backend requires a Linux host".to_string(),
                    })
                }
            }
            BackendKind::MacosScreenCaptureKit => {
                #[cfg(all(target_os = "macos", feature = "macos-sck"))]
                {
                    Ok(Box::new(crate::macos::MacosScreenCaptureKitBackend::new()))
                }
                #[cfg(not(all(target_os = "macos", feature = "macos-sck")))]
                {
                    Err(CaptureError::Unsupported {
                        message:
                            "macos-sck backend requires a macOS host built with the macos-sck feature"
                                .to_string(),
                    })
                }
            }
            BackendKind::Unsupported => Err(CaptureError::Unsupported {
                message: format!(
                    "no capture backend is available on os={} session={}",
                    std::env::consts::OS,
                    std::env::var("XDG_SESSION_TYPE").unwrap_or_default()
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
        "macos" => {
            #[cfg(feature = "macos-sck")]
            {
                BackendKind::MacosScreenCaptureKit
            }
            #[cfg(not(feature = "macos-sck"))]
            {
                BackendKind::Unsupported
            }
        }
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
            assert_eq!(BackendKind::from_cli_flag(kind.as_flag()).unwrap(), kind);
        }
    }

    #[test]
    fn default_backend_for_decision_matrix() {
        let expected_macos_backend = if cfg!(feature = "macos-sck") {
            BackendKind::MacosScreenCaptureKit
        } else {
            BackendKind::Unsupported
        };
        assert_eq!(default_backend_for("macos", None), expected_macos_backend);
        assert_eq!(
            default_backend_for("macos", Some("wayland")),
            expected_macos_backend
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
        assert_eq!(
            default_backend_for("windows", None),
            BackendKind::Unsupported
        );
    }

    #[test]
    fn fixture_kind_create_requires_path() {
        match BackendKind::Fixture.create() {
            Err(CaptureError::InvalidConfig { message }) => {
                assert!(message.contains("--fixture"), "msg = {message}");
            }
            Err(other) => panic!("expected InvalidConfig, got {other:?}"),
            Ok(_) => panic!("expected error, got Ok"),
        }
    }

    #[test]
    fn unsupported_kind_create_returns_unsupported() {
        match BackendKind::Unsupported.create() {
            Err(CaptureError::Unsupported { .. }) => {}
            Err(other) => panic!("expected Unsupported, got {other:?}"),
            Ok(_) => panic!("expected error, got Ok"),
        }
    }
}
