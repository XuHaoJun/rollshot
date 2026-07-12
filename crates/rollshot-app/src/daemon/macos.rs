//! macOS daemon adapter.
pub(crate) mod shortcut;
pub(crate) mod tray;

use crate::daemon::config::DaemonConfig;
use crate::daemon::core::{DaemonCore, DaemonEvent, LoopAction};
use crate::daemon::process::CurrentExeLauncher;
use crate::daemon::start_parts;
use std::sync::mpsc::Receiver;
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
use winit::platform::macos::{ActivationPolicy, EventLoopBuilderExtMacOS};
use winit::window::WindowId;

/// The daemon's winit application. Holds the shared core plus the platform
/// guards; the guards are created in `resumed` (after the NSApplication has
/// launched) and dropped when the loop exits.
///
/// Field declaration order **is** the drop order and must match the documented
/// teardown (see "Ownership and shutdown order"): `core` first (so an active
/// capture group is terminated before any UI resource is torn down), then
/// `shortcut`, then `tray`. The shortcut/tray order is functionally immaterial
/// (independent process-global handlers), but is kept consistent with the
/// diagram so the doc never drifts from the code.
struct DaemonApp {
    core: DaemonCore<CurrentExeLauncher>,
    config: DaemonConfig,
    proxy: EventLoopProxy<DaemonEvent>,
    shortcut: Option<shortcut::ShortcutGuard>,
    tray: Option<tray::TrayGuard>,
    started: bool,
    startup_error: Option<String>,
}

impl ApplicationHandler<DaemonEvent> for DaemonApp {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.started {
            return;
        }
        self.started = true;
        event_loop.set_control_flow(ControlFlow::Wait);

        let proxy = self.proxy.clone();
        let region_hotkey = self.config.capture_region_hotkey.clone();
        #[cfg(feature = "ocr")]
        let text_hotkey = self.config.capture_text_hotkey.clone();
        match start_parts(
            || tray::TrayGuard::start(proxy.clone()),
            || {
                shortcut::ShortcutGuard::start(
                    proxy.clone(),
                    &region_hotkey,
                    #[cfg(feature = "ocr")]
                    text_hotkey.as_ref(),
                )
            },
        ) {
            Ok((tray, shortcut)) => {
                self.tray = Some(tray);
                self.shortcut = shortcut;
                let text_shortcut_display = {
                    #[cfg(feature = "ocr")]
                    {
                        self.config
                            .capture_text_hotkey
                            .as_ref()
                            .map(|s| s.to_string())
                            .unwrap_or_default()
                    }
                    #[cfg(not(feature = "ocr"))]
                    {
                        String::new()
                    }
                };
                tracing::info!(
                    target: "rollshot::daemon::core",
                    version = env!("CARGO_PKG_VERSION"),
                    os = std::env::consts::OS,
                    preferred_shortcut = %self.config.capture_region_hotkey,
                    text_shortcut = %text_shortcut_display,
                    shortcut_active = self.shortcut.is_some(),
                    "Rollshot tray daemon ready"
                );
            }
            Err(error) => {
                // Tray init failure is fatal (spec): record it and exit; the
                // outer `run` surfaces it as an error.
                self.startup_error = Some(error);
                event_loop.exit();
            }
        }
    }

    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: DaemonEvent) {
        if self.core.handle(event) == LoopAction::Exit {
            event_loop.exit();
        }
    }

    fn window_event(&mut self, _event_loop: &ActiveEventLoop, _id: WindowId, _event: WindowEvent) {
        // The daemon owns no windows; capture runs as a child process.
    }
}

/// Run the macOS daemon on the main thread. Owns the winit loop and the
/// forwarder that bridges watcher-thread `CaptureExited` events into it. Returns
/// when the user quits (clean teardown via guard `Drop`) or with the fatal tray
/// error if startup failed.
pub fn run(
    core: DaemonCore<CurrentExeLauncher>,
    capture_exits: Receiver<DaemonEvent>,
    config: &DaemonConfig,
) -> Result<(), String> {
    let event_loop = EventLoop::<DaemonEvent>::with_user_event()
        .with_activation_policy(ActivationPolicy::Accessory)
        .build()
        .map_err(|error| format!("failed to build macOS event loop: {error}"))?;
    let proxy = event_loop.create_proxy();

    // Bridge the capture watcher thread (which sends `CaptureExited` over the
    // core's mpsc sender) into the main-thread loop. Detached; it ends when the
    // sender (owned by the core) drops or the loop closes.
    let forward = proxy.clone();
    std::thread::Builder::new()
        .name("rollshot-daemon-forward".into())
        .spawn(move || {
            while let Ok(event) = capture_exits.recv() {
                if forward.send_event(event).is_err() {
                    break;
                }
            }
        })
        .map_err(|error| format!("failed to start daemon forwarder thread: {error}"))?;

    let mut app = DaemonApp {
        core,
        config: config.clone(),
        proxy,
        tray: None,
        shortcut: None,
        started: false,
        startup_error: None,
    };
    event_loop
        .run_app(&mut app)
        .map_err(|error| format!("macOS daemon event loop failed: {error}"))?;

    if let Some(error) = app.startup_error.take() {
        return Err(error);
    }
    Ok(())
}
