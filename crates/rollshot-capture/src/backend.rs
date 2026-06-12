use crate::diagnostics::TARGET_CAPTURE;
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
        let result = match flag {
            "auto" => Ok(default_backend()),
            "fixture" => Ok(BackendKind::Fixture),
            "linux-portal" => Ok(BackendKind::LinuxPortalPipeWire),
            "macos-sck" => Ok(BackendKind::MacosScreenCaptureKit),
            other => Err(CaptureError::InvalidConfig {
                message: format!(
                    "unknown backend '{other}'; expected one of: auto, fixture, linux-portal, macos-sck"
                ),
            }),
        };
        match &result {
            Ok(kind) => {
                tracing::debug!(target: TARGET_CAPTURE, flag, kind = kind.as_flag(), "backend flag resolved")
            }
            Err(e) => {
                tracing::error!(target: TARGET_CAPTURE, flag, error = %e, "backend flag rejected")
            }
        }
        result
    }

    pub fn create(self) -> Result<Box<dyn CaptureBackend>, CaptureError> {
        match self {
            BackendKind::Fixture => {
                let err = CaptureError::InvalidConfig {
                    message: "fixture backend requires --fixture <DIR>".to_string(),
                };
                tracing::error!(target: TARGET_CAPTURE, kind = self.as_flag(), error = %err, "backend creation failed");
                Err(err)
            }
            BackendKind::LinuxPortalPipeWire => {
                #[cfg(target_os = "linux")]
                {
                    tracing::debug!(target: TARGET_CAPTURE, kind = self.as_flag(), "backend created");
                    Ok(Box::new(crate::linux::LinuxPortalBackend::new()))
                }
                #[cfg(not(target_os = "linux"))]
                {
                    let err = CaptureError::Unsupported {
                        message: "linux-portal backend requires a Linux host".to_string(),
                    };
                    tracing::warn!(target: TARGET_CAPTURE, kind = self.as_flag(), error = %err, "backend unsupported");
                    Err(err)
                }
            }
            BackendKind::MacosScreenCaptureKit => {
                #[cfg(target_os = "macos")]
                {
                    tracing::debug!(target: TARGET_CAPTURE, kind = self.as_flag(), "backend created");
                    Ok(Box::new(crate::macos::MacosScreenCaptureKitBackend::new()))
                }
                #[cfg(not(target_os = "macos"))]
                {
                    let err = CaptureError::Unsupported {
                        message: "macos-sck backend requires a macOS host".to_string(),
                    };
                    tracing::warn!(target: TARGET_CAPTURE, kind = self.as_flag(), error = %err, "backend unsupported");
                    Err(err)
                }
            }
            BackendKind::Unsupported => {
                let err = CaptureError::Unsupported {
                    message: format!(
                        "no capture backend is available on os={} session={}",
                        std::env::consts::OS,
                        std::env::var("XDG_SESSION_TYPE").unwrap_or_default()
                    ),
                };
                tracing::warn!(target: TARGET_CAPTURE, kind = self.as_flag(), "backend unsupported");
                Err(err)
            }
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
    use std::io::Write;
    use std::sync::{Arc, Mutex};
    use tracing_subscriber::fmt::MakeWriter;

    #[derive(Clone, Default)]
    struct LogWriter(Arc<Mutex<Vec<u8>>>);

    struct LogGuard(Arc<Mutex<Vec<u8>>>);

    impl Write for LogGuard {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(buf);
            Ok(buf.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    impl<'a> MakeWriter<'a> for LogWriter {
        type Writer = LogGuard;

        fn make_writer(&'a self) -> Self::Writer {
            LogGuard(Arc::clone(&self.0))
        }
    }

    fn capture_logs(run: impl FnOnce()) -> String {
        let writer = LogWriter::default();
        let subscriber = tracing_subscriber::fmt()
            .without_time()
            .with_ansi(false)
            .with_max_level(tracing::Level::WARN)
            .with_writer(writer.clone())
            .finish();
        tracing::subscriber::with_default(subscriber, run);
        let bytes = writer.0.lock().unwrap().clone();
        String::from_utf8(bytes).unwrap()
    }

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
        assert_eq!(
            default_backend_for("windows", None),
            BackendKind::Unsupported
        );
    }

    #[test]
    fn unsupported_backend_event_omits_session_value() {
        let _guard = crate::ENV_MUTEX.lock().unwrap();
        let session = "private-custom-session";
        std::env::set_var("XDG_SESSION_TYPE", session);
        let log = capture_logs(|| {
            assert!(BackendKind::Unsupported.create().is_err());
        });
        std::env::remove_var("XDG_SESSION_TYPE");

        assert!(log.contains("backend unsupported"), "log = {log}");
        assert!(!log.contains(session), "log = {log}");
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
