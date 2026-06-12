#![cfg(target_os = "linux")]

pub mod kwin_screenshot;
pub mod kwin_screencast;
pub mod one_shot;
mod pipewire;
mod pixel;
mod portal;
pub(crate) mod portal_screenshot;

use crate::backend::{CaptureBackend, FrameStream};
use crate::diagnostics::TARGET_CAPTURE;
use crate::error::CaptureError;
use crate::types::{CaptureOptions, CaptureProbe, RegionMode};

use pipewire::LinuxPortalFrameStream;
use portal::{PortalClient, PortalSession};

pub(crate) trait PortalBehavior: Send {
    fn probe(&self) -> CaptureProbe;
    fn start(&self, options: CaptureOptions) -> Result<PortalSession, CaptureError>;
}

impl PortalBehavior for PortalClient {
    fn probe(&self) -> CaptureProbe {
        PortalClient::probe(self)
    }

    fn start(&self, options: CaptureOptions) -> Result<PortalSession, CaptureError> {
        PortalClient::start(self, options)
    }
}

pub struct LinuxPortalBackend {
    portal: Box<dyn PortalBehavior>,
}

impl LinuxPortalBackend {
    pub fn new() -> Self {
        Self {
            portal: Box::new(PortalClient::new()),
        }
    }

    #[cfg(test)]
    pub(crate) fn with_portal(portal: impl PortalBehavior + 'static) -> Self {
        Self {
            portal: Box::new(portal),
        }
    }
}

impl Default for LinuxPortalBackend {
    fn default() -> Self {
        Self::new()
    }
}

fn validate_manual_crop(
    region: &crate::types::Region,
    frame_width: u32,
    frame_height: u32,
) -> Result<(), CaptureError> {
    if region.x < 0 || region.y < 0 {
        return Err(CaptureError::InvalidConfig {
            message: "region x and y must be non-negative".to_string(),
        });
    }
    if frame_width > 0 && frame_height > 0 {
        let x2 = region.x as u32 + region.width;
        let y2 = region.y as u32 + region.height;
        if x2 > frame_width || y2 > frame_height {
            return Err(CaptureError::InvalidConfig {
                message: format!(
                    "manual crop region x={},y={},w={},h={} is outside post-VideoCrop frame {}x{}",
                    region.x, region.y, region.width, region.height, frame_width, frame_height
                ),
            });
        }
    }
    Ok(())
}

impl CaptureBackend for LinuxPortalBackend {
    fn name(&self) -> &'static str {
        "linux-portal"
    }

    fn probe(&self) -> CaptureProbe {
        self.portal.probe()
    }

    fn start(&mut self, options: CaptureOptions) -> Result<Box<dyn FrameStream>, CaptureError> {
        let region_category = match &options.region {
            RegionMode::Manual(_) => "manual",
            RegionMode::PortalPicker => "portal-picker",
            RegionMode::FullSource => "full-source",
        };
        tracing::info!(
            target: TARGET_CAPTURE,
            backend = "linux-portal",
            fps = options.fps,
            show_cursor = options.show_cursor,
            region = region_category,
            target_display = options.target_display_id.is_some(),
            "capture start requested"
        );

        let session = self.portal.start(options.clone())?;

        if let RegionMode::Manual(region) = &options.region {
            validate_manual_crop(region, session.frame_width, session.frame_height)?;
        }

        tracing::debug!(target: TARGET_CAPTURE, "connecting PipeWire stream");
        let stream = LinuxPortalFrameStream::connect(session, options)?;
        Ok(Box::new(stream))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::CaptureBackend;
    use crate::types::{CaptureOptions, Region, RegionMode};

    use portal::PortalSession;

    use std::cell::{Cell, RefCell};
    use std::sync::MutexGuard;

    thread_local! {
        static ENV_LOCK_DEPTH: Cell<u32> = const { Cell::new(0) };
        static ENV_GUARD: RefCell<Option<MutexGuard<'static, ()>>> = const { RefCell::new(None) };
    }

    fn acquire_env_lock() {
        ENV_LOCK_DEPTH.with(|depth| {
            if depth.get() == 0 {
                ENV_GUARD.with(|guard| {
                    *guard.borrow_mut() = Some(crate::ENV_MUTEX.lock().unwrap());
                });
            }
            depth.set(depth.get() + 1);
        });
    }

    fn release_env_lock() {
        ENV_LOCK_DEPTH.with(|depth| {
            let d = depth.get() - 1;
            if d == 0 {
                ENV_GUARD.with(|guard| {
                    *guard.borrow_mut() = None;
                });
            }
            depth.set(d);
        });
    }

    struct EnvGuard {
        key: &'static str,
        original: Option<String>,
    }

    impl EnvGuard {
        fn set(key: &'static str, value: &str) -> Self {
            acquire_env_lock();
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
            release_env_lock();
        }
    }

    fn fake_portal_fd() -> std::os::fd::OwnedFd {
        std::fs::File::open("/dev/null")
            .expect("open /dev/null")
            .into()
    }

    struct FakePortal {
        session_factory: Box<dyn Fn() -> Result<PortalSession, CaptureError> + Send>,
    }

    impl PortalBehavior for FakePortal {
        fn probe(&self) -> CaptureProbe {
            CaptureProbe {
                backend: "linux-portal",
                available: true,
                message: "fake".to_string(),
                details: vec![],
            }
        }

        fn start(&self, _options: CaptureOptions) -> Result<PortalSession, CaptureError> {
            (self.session_factory)()
        }
    }

    fn make_session(frame_width: u32, frame_height: u32) -> PortalSession {
        PortalSession::new_for_test(
            42,
            fake_portal_fd(),
            portal::LinuxPortalCapabilities {
                desktop: "test".to_string(),
                session_type: "wayland".to_string(),
                portal_version: Some(4),
                source_types: portal::SourceTypes {
                    monitor: true,
                    window: true,
                    virtual_source: false,
                },
                cursor_modes: portal::CursorModes {
                    hidden: true,
                    embedded: true,
                    metadata: false,
                },
                profile: portal::LinuxDesktopProfile::Unknown,
                quirks: Vec::new(),
            },
            frame_width,
            frame_height,
        )
    }

    fn make_options(region: RegionMode) -> CaptureOptions {
        CaptureOptions {
            region,
            fps: 5,
            show_cursor: false,
            prefer_portal_region: true,
            target_display_id: None,
            target_output_name: None,
        }
    }

    #[test]
    fn name_returns_linux_portal() {
        let backend = LinuxPortalBackend::with_portal(FakePortal {
            session_factory: Box::new(|| Ok(make_session(0, 0))),
        });
        assert_eq!(backend.name(), "linux-portal");
    }

    #[test]
    fn start_portal_picker_passes_no_manual_crop() {
        let _guard = EnvGuard::set("XDG_SESSION_TYPE", "wayland");
        let mut backend = LinuxPortalBackend::with_portal(FakePortal {
            session_factory: Box::new(|| Ok(make_session(1920, 1080))),
        });
        let opts = make_options(RegionMode::PortalPicker);
        let result = backend.start(opts);
        assert!(result.is_ok(), "expected Ok from start");
        let captured = pipewire::connection::take_captured_options();
        assert!(captured.is_some(), "expected PipeWire to receive options");
        assert_eq!(captured.unwrap().region, RegionMode::PortalPicker);
    }

    #[test]
    fn start_full_source_passes_no_manual_crop() {
        let _guard = EnvGuard::set("XDG_SESSION_TYPE", "wayland");
        let mut backend = LinuxPortalBackend::with_portal(FakePortal {
            session_factory: Box::new(|| Ok(make_session(1920, 1080))),
        });
        let opts = make_options(RegionMode::FullSource);
        let result = backend.start(opts);
        assert!(result.is_ok(), "expected Ok from start");
        let captured = pipewire::connection::take_captured_options();
        assert!(captured.is_some(), "expected PipeWire to receive options");
        assert_eq!(captured.unwrap().region, RegionMode::FullSource);
    }

    #[test]
    fn start_manual_passes_local_crop() {
        let _guard = EnvGuard::set("XDG_SESSION_TYPE", "wayland");
        let mut backend = LinuxPortalBackend::with_portal(FakePortal {
            session_factory: Box::new(|| Ok(make_session(1000, 800))),
        });
        let region = Region {
            x: 100,
            y: 50,
            width: 200,
            height: 150,
        };
        let opts = make_options(RegionMode::Manual(region));
        let result = backend.start(opts);
        assert!(result.is_ok(), "expected Ok from start");
        let captured = pipewire::connection::take_captured_options();
        assert!(captured.is_some(), "expected PipeWire to receive options");
        assert_eq!(captured.unwrap().region, RegionMode::Manual(region));
    }

    #[test]
    fn start_manual_outside_frame_returns_invalid_config() {
        let _guard = EnvGuard::set("XDG_SESSION_TYPE", "wayland");
        let mut backend = LinuxPortalBackend::with_portal(FakePortal {
            session_factory: Box::new(|| Ok(make_session(1000, 800))),
        });
        let region = Region {
            x: 500,
            y: 0,
            width: 600,
            height: 800,
        };
        let opts = make_options(RegionMode::Manual(region));
        match backend.start(opts) {
            Err(CaptureError::InvalidConfig { message }) => {
                assert!(
                    message.contains("1000x800"),
                    "expected frame size in message: {message}"
                );
                assert!(
                    message.contains("500") && message.contains("600"),
                    "expected region in message: {message}"
                );
            }
            Err(e) => panic!("expected InvalidConfig, got Err: {e}"),
            Ok(_) => panic!("expected InvalidConfig, got Ok"),
        }
    }

    #[test]
    fn start_manual_negative_coords_returns_invalid_config() {
        let _guard = EnvGuard::set("XDG_SESSION_TYPE", "wayland");
        let mut backend = LinuxPortalBackend::with_portal(FakePortal {
            session_factory: Box::new(|| Ok(make_session(1000, 800))),
        });
        let region = Region {
            x: -10,
            y: 0,
            width: 100,
            height: 100,
        };
        let opts = make_options(RegionMode::Manual(region));
        match backend.start(opts) {
            Err(CaptureError::InvalidConfig { message }) => {
                assert!(
                    message.contains("non-negative"),
                    "expected non-negative message: {message}"
                );
            }
            Err(e) => panic!("expected InvalidConfig, got Err: {e}"),
            Ok(_) => panic!("expected InvalidConfig, got Ok"),
        }
    }

    #[test]
    fn start_manual_exact_fit_succeeds() {
        let _guard = EnvGuard::set("XDG_SESSION_TYPE", "wayland");
        let mut backend = LinuxPortalBackend::with_portal(FakePortal {
            session_factory: Box::new(|| Ok(make_session(1000, 800))),
        });
        let region = Region {
            x: 0,
            y: 0,
            width: 1000,
            height: 800,
        };
        let opts = make_options(RegionMode::Manual(region));
        let result = backend.start(opts);
        assert!(result.is_ok(), "expected Ok from start");
    }

    #[test]
    fn start_manual_zero_dimensions_skips_bounds_check() {
        let _guard = EnvGuard::set("XDG_SESSION_TYPE", "wayland");
        let mut backend = LinuxPortalBackend::with_portal(FakePortal {
            session_factory: Box::new(|| Ok(make_session(0, 0))),
        });
        let region = Region {
            x: 0,
            y: 0,
            width: 9999,
            height: 9999,
        };
        let opts = make_options(RegionMode::Manual(region));
        let result = backend.start(opts);
        assert!(result.is_ok(), "expected Ok when frame dims unknown");
    }

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
        let _guard = EnvGuard::set("XDG_SESSION_TYPE", "wayland");
        let result = backend.start(CaptureOptions::default());
        assert!(result.is_ok(), "expected Ok from start");
    }
}
