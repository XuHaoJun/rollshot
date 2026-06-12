//! Native-first Linux capture backend with portal fallback.
//!
//! Tries KWin screencast on KDE Wayland; falls back to the XDG portal for
//! non-KDE Wayland or when the native backend fails with an eligible error.
//!
//! ```text
//! auto (Linux Wayland)
//!         │
//!    KDE detected? ──no──► start linux-portal
//!         │ yes
//!    strict linux-kwin startup
//!         │
//!    ok? ──yes──► native stream
//!         │ no
//!    fallback-eligible? ──no──► return native error
//!         │ yes
//!    warn (target rollshot::capture::linux::kwin)
//!         │
//!    start linux-portal ──err──► combined native+portal error
//! ```

use crate::backend::{CaptureBackend, FrameStream};
use crate::diagnostics::{TARGET_CAPTURE, TARGET_LINUX_KWIN};
use crate::error::CaptureError;
use crate::types::{CaptureOptions, CaptureProbe};

use super::{LinuxKwinBackend, LinuxPortalBackend};

type NativeFactory = Box<dyn Fn() -> Box<dyn CaptureBackend> + Send>;
type PortalFactory = Box<dyn Fn() -> Box<dyn CaptureBackend> + Send>;

pub struct LinuxAutoBackend {
    native_factory: NativeFactory,
    portal_factory: PortalFactory,
    is_kde: bool,
    session_type: Option<String>,
}

impl LinuxAutoBackend {
    pub fn new() -> Self {
        let is_kde = std::env::var("XDG_CURRENT_DESKTOP")
            .ok()
            .is_some_and(|d| d.split(':').any(|p| p.eq_ignore_ascii_case("KDE")));

        Self {
            native_factory: Box::new(|| {
                Box::new(LinuxKwinBackend::new(
                    super::kwin_screencast::RealKwinScreencastClient::new(),
                    None,
                ))
            }),
            portal_factory: Box::new(|| Box::new(LinuxPortalBackend::new())),
            is_kde,
            session_type: None,
        }
    }

    #[cfg(test)]
    fn with_factories(
        native_factory: NativeFactory,
        portal_factory: PortalFactory,
        is_kde: bool,
    ) -> Self {
        Self {
            native_factory,
            portal_factory,
            is_kde,
            session_type: Some("wayland".to_string()),
        }
    }
}

impl Default for LinuxAutoBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl CaptureBackend for LinuxAutoBackend {
    fn name(&self) -> &'static str {
        "linux-auto"
    }

    fn probe(&self) -> CaptureProbe {
        let session_type = self
            .session_type
            .clone()
            .or_else(|| std::env::var("XDG_SESSION_TYPE").ok())
            .unwrap_or_default();
        let is_wayland = session_type == "wayland";
        CaptureProbe {
            backend: "linux-auto",
            available: is_wayland,
            message: if is_wayland {
                String::new()
            } else {
                "linux-auto requires a Wayland session".to_string()
            },
            details: vec![("XDG_SESSION_TYPE".to_string(), session_type)],
        }
    }

    fn start(&mut self, options: CaptureOptions) -> Result<Box<dyn FrameStream>, CaptureError> {
        let session_type = self
            .session_type
            .clone()
            .or_else(|| std::env::var("XDG_SESSION_TYPE").ok());
        if session_type.as_deref() != Some("wayland") {
            return Err(CaptureError::Unsupported {
                message: "linux-auto requires Wayland".to_string(),
            });
        }

        if self.is_kde {
            let mut native = (self.native_factory)();
            match native.start(options.clone()) {
                Ok(stream) => {
                    tracing::debug!(
                        target: TARGET_CAPTURE,
                        backend = "linux-kwin",
                        "native KWin capture started"
                    );
                    Ok(stream)
                }
                Err(native_error) => {
                    if !is_fallback_eligible(&native_error) {
                        return Err(native_error);
                    }

                    tracing::warn!(
                        target: TARGET_LINUX_KWIN,
                        reason = fallback_reason(&native_error),
                        fallback = "linux-portal",
                        error = %native_error,
                        "KWin native capture unavailable; falling back to portal"
                    );

                    let mut portal = (self.portal_factory)();
                    match portal.start(options) {
                        Ok(stream) => {
                            tracing::debug!(
                                target: TARGET_CAPTURE,
                                backend = "linux-portal",
                                "portal fallback capture started"
                            );
                            Ok(stream)
                        }
                        Err(portal_error) => {
                            Err(combine_fallback_errors(native_error, portal_error))
                        }
                    }
                }
            }
        } else {
            let mut portal = (self.portal_factory)();
            portal.start(options)
        }
    }
}

pub fn is_fallback_eligible(error: &CaptureError) -> bool {
    matches!(
        error,
        CaptureError::Unsupported { .. }
            | CaptureError::PermissionDenied { .. }
            | CaptureError::Timeout { .. }
            | CaptureError::Mapping { .. }
            | CaptureError::Backend(_)
    )
}

fn fallback_reason(error: &CaptureError) -> &'static str {
    match error {
        CaptureError::Unsupported { .. } => "unsupported",
        CaptureError::PermissionDenied { .. } => "permission-denied",
        CaptureError::Timeout { .. } => "timeout",
        CaptureError::Mapping { .. } => "mapping",
        CaptureError::Backend(_) => "backend-error",
        _ => "unknown",
    }
}

fn combine_fallback_errors(native: CaptureError, portal: CaptureError) -> CaptureError {
    CaptureError::Backend(anyhow::anyhow!(
        "native capture failed: {native}; portal fallback also failed: {portal}"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::{CaptureBackend, FrameStream};
    use crate::types::CapturedFrame;

    use std::sync::{Arc, Mutex};

    #[derive(Clone, Default)]
    struct Calls {
        native: Arc<Mutex<u32>>,
        portal: Arc<Mutex<u32>>,
    }

    impl Calls {
        fn inc_native(&self) {
            *self.native.lock().unwrap() += 1;
        }

        fn inc_portal(&self) {
            *self.portal.lock().unwrap() += 1;
        }

        fn native(&self) -> u32 {
            *self.native.lock().unwrap()
        }

        fn portal(&self) -> u32 {
            *self.portal.lock().unwrap()
        }
    }

    struct RecordingStream;

    impl FrameStream for RecordingStream {
        fn next_frame(&mut self) -> Result<CapturedFrame, CaptureError> {
            Err(CaptureError::EndOfStream)
        }
    }

    enum BackendAction {
        Ok,
        Err(CaptureError),
    }

    struct CallRecordingBackend {
        name: &'static str,
        action: BackendAction,
        calls: Calls,
        is_native: bool,
    }

    impl CaptureBackend for CallRecordingBackend {
        fn name(&self) -> &'static str {
            self.name
        }

        fn probe(&self) -> CaptureProbe {
            CaptureProbe {
                backend: self.name,
                available: true,
                message: String::new(),
                details: vec![],
            }
        }

        fn start(
            &mut self,
            _options: CaptureOptions,
        ) -> Result<Box<dyn FrameStream>, CaptureError> {
            if self.is_native {
                self.calls.inc_native();
            } else {
                self.calls.inc_portal();
            }
            match &self.action {
                BackendAction::Ok => Ok(Box::new(RecordingStream)),
                BackendAction::Err(e) => Err(clone_error(e)),
            }
        }
    }

    fn clone_error(e: &CaptureError) -> CaptureError {
        match e {
            CaptureError::Unsupported { message } => CaptureError::Unsupported {
                message: message.clone(),
            },
            CaptureError::PermissionDenied { message } => CaptureError::PermissionDenied {
                message: message.clone(),
            },
            CaptureError::UserCancelled => CaptureError::UserCancelled,
            CaptureError::EndOfStream => CaptureError::EndOfStream,
            CaptureError::Timeout { message, duration } => CaptureError::Timeout {
                message: message.clone(),
                duration: *duration,
            },
            CaptureError::InvalidConfig { message } => CaptureError::InvalidConfig {
                message: message.clone(),
            },
            CaptureError::Mapping { message } => CaptureError::Mapping {
                message: message.clone(),
            },
            CaptureError::Backend(e) => CaptureError::Backend(anyhow::anyhow!("{}", e)),
            CaptureError::NotImplemented { backend } => CaptureError::NotImplemented { backend },
        }
    }

    fn native_ok(calls: &Calls) -> Box<dyn Fn() -> Box<dyn CaptureBackend> + Send> {
        let c = calls.clone();
        Box::new(move || {
            Box::new(CallRecordingBackend {
                name: "linux-kwin",
                action: BackendAction::Ok,
                calls: c.clone(),
                is_native: true,
            })
        })
    }

    fn native_err(
        calls: &Calls,
        err: CaptureError,
    ) -> Box<dyn Fn() -> Box<dyn CaptureBackend> + Send> {
        let c = calls.clone();
        Box::new(move || {
            Box::new(CallRecordingBackend {
                name: "linux-kwin",
                action: BackendAction::Err(clone_error(&err)),
                calls: c.clone(),
                is_native: true,
            })
        })
    }

    fn portal_ok(calls: &Calls) -> Box<dyn Fn() -> Box<dyn CaptureBackend> + Send> {
        let c = calls.clone();
        Box::new(move || {
            Box::new(CallRecordingBackend {
                name: "linux-portal",
                action: BackendAction::Ok,
                calls: c.clone(),
                is_native: false,
            })
        })
    }

    fn native_stream_that_fails_on_next_frame(
        calls: &Calls,
    ) -> Box<dyn Fn() -> Box<dyn CaptureBackend> + Send> {
        let c = calls.clone();
        Box::new(move || Box::new(FailingStreamBackend { calls: c.clone() }))
    }

    struct FailingStreamBackend {
        calls: Calls,
    }

    struct FailingStream;

    impl FrameStream for FailingStream {
        fn next_frame(&mut self) -> Result<CapturedFrame, CaptureError> {
            Err(CaptureError::Backend(anyhow::anyhow!("stream broken")))
        }
    }

    impl CaptureBackend for FailingStreamBackend {
        fn name(&self) -> &'static str {
            "linux-kwin"
        }

        fn probe(&self) -> CaptureProbe {
            CaptureProbe {
                backend: "linux-kwin",
                available: true,
                message: String::new(),
                details: vec![],
            }
        }

        fn start(
            &mut self,
            _options: CaptureOptions,
        ) -> Result<Box<dyn FrameStream>, CaptureError> {
            self.calls.inc_native();
            Ok(Box::new(FailingStream))
        }
    }

    struct RecordingKwinClient {
        log: Arc<Mutex<Vec<&'static str>>>,
    }

    impl crate::linux::kwin_screencast::KwinScreencastClient for RecordingKwinClient {
        fn start_output(
            &self,
            output_name: &str,
            _show_cursor: bool,
        ) -> Result<crate::linux::kwin_screencast::KwinScreencastSession, CaptureError> {
            self.log
                .lock()
                .unwrap()
                .push(Box::leak(output_name.to_string().into_boxed_str()));
            Err(CaptureError::Backend(anyhow::anyhow!("test stub")))
        }
    }

    fn recording_kwin_client() -> (RecordingKwinClient, Arc<Mutex<Vec<&'static str>>>) {
        let log = Arc::new(Mutex::new(Vec::new()));
        (
            RecordingKwinClient {
                log: Arc::clone(&log),
            },
            log,
        )
    }

    fn test_auto_backend(
        native: Box<dyn Fn() -> Box<dyn CaptureBackend> + Send>,
        portal: Box<dyn Fn() -> Box<dyn CaptureBackend> + Send>,
    ) -> LinuxAutoBackend {
        LinuxAutoBackend::with_factories(native, portal, true)
    }

    fn test_kwin_backend(
        client: impl crate::linux::kwin_screencast::KwinScreencastClient + 'static,
        resolver: Option<Box<dyn Fn() -> Result<String, CaptureError> + Send>>,
    ) -> LinuxKwinBackend {
        LinuxKwinBackend::new(client, resolver)
    }

    fn failing_kwin_client() -> impl crate::linux::kwin_screencast::KwinScreencastClient {
        struct Fail;
        impl crate::linux::kwin_screencast::KwinScreencastClient for Fail {
            fn start_output(
                &self,
                _: &str,
                _: bool,
            ) -> Result<crate::linux::kwin_screencast::KwinScreencastSession, CaptureError>
            {
                Err(CaptureError::Backend(anyhow::anyhow!("kwin failed")))
            }
        }
        Fail
    }

    fn active_output_resolver(
        name: &'static str,
    ) -> Option<Box<dyn Fn() -> Result<String, CaptureError> + Send>> {
        Some(Box::new(move || Ok(name.to_string())))
    }

    fn targeted_options(target: &str) -> CaptureOptions {
        CaptureOptions {
            target_output_name: Some(target.to_string()),
            ..CaptureOptions::default()
        }
    }

    fn native_failure() -> CaptureError {
        CaptureError::PermissionDenied {
            message: "native denied".into(),
        }
    }

    fn portal_failure() -> CaptureError {
        CaptureError::PermissionDenied {
            message: "portal denied".into(),
        }
    }

    #[test]
    fn native_success_skips_portal() {
        let calls = Calls::default();
        let mut backend = test_auto_backend(native_ok(&calls), portal_ok(&calls));
        backend.start(targeted_options("eDP-1")).unwrap();
        assert_eq!(calls.native(), 1);
        assert_eq!(calls.portal(), 0);
    }

    #[test]
    fn eligible_native_failure_starts_portal_once() {
        let calls = Calls::default();
        let mut backend = test_auto_backend(
            native_err(
                &calls,
                CaptureError::PermissionDenied {
                    message: "denied".into(),
                },
            ),
            portal_ok(&calls),
        );
        backend.start(targeted_options("eDP-1")).unwrap();
        assert_eq!(calls.portal(), 1);
    }

    #[test]
    fn user_cancelled_never_falls_back() {
        let calls = Calls::default();
        let mut backend = test_auto_backend(
            native_err(&calls, CaptureError::UserCancelled),
            portal_ok(&calls),
        );
        assert!(matches!(
            backend.start(targeted_options("eDP-1")),
            Err(CaptureError::UserCancelled)
        ));
        assert_eq!(calls.portal(), 0);
    }

    #[test]
    fn explicit_kwin_backend_never_constructs_portal() {
        let mut backend = LinuxKwinBackend::new(failing_kwin_client(), None);
        assert!(backend.start(targeted_options("eDP-1")).is_err());
    }

    #[test]
    fn both_failures_preserve_native_and_portal_context() {
        let err = combine_fallback_errors(native_failure(), portal_failure());
        let text = err.to_string();
        assert!(text.contains("native"));
        assert!(text.contains("portal"));
    }

    #[test]
    fn native_runtime_stream_error_does_not_construct_portal() {
        let calls = Calls::default();
        let mut backend = test_auto_backend(
            native_stream_that_fails_on_next_frame(&calls),
            portal_ok(&calls),
        );
        let mut stream = backend.start(targeted_options("eDP-1")).unwrap();
        assert!(stream.next_frame().is_err());
        assert_eq!(calls.portal(), 0);
    }

    #[test]
    fn explicit_kwin_backend_resolves_active_output_when_target_is_missing() {
        let (client, log) = recording_kwin_client();
        let mut backend = test_kwin_backend(client, active_output_resolver("eDP-1"));
        let _ = backend.start(CaptureOptions::default());
        assert_eq!(log.lock().unwrap().first().copied(), Some("eDP-1"));
    }

    #[test]
    fn user_cancelled_and_invalid_config_are_not_fallback_eligible() {
        assert!(!is_fallback_eligible(&CaptureError::UserCancelled));
        assert!(!is_fallback_eligible(&CaptureError::InvalidConfig {
            message: "bad".into(),
        }));
    }

    #[test]
    fn non_kde_skips_native_entirely() {
        let calls = Calls::default();
        let mut backend =
            LinuxAutoBackend::with_factories(native_ok(&calls), portal_ok(&calls), false);
        backend.start(targeted_options("eDP-1")).unwrap();
        assert_eq!(calls.native(), 0);
        assert_eq!(calls.portal(), 1);
    }
}
