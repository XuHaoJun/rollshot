pub(crate) mod shortcut;
pub(crate) mod tray;

use crate::daemon::config::DaemonConfig;
use crate::daemon::core::DaemonEvent;

pub struct LinuxPlatform {
    _shortcut: Option<shortcut::ShortcutGuard>,
    _tray: tray::TrayGuard,
}

impl LinuxPlatform {
    pub fn start(
        events: std::sync::mpsc::Sender<DaemonEvent>,
        config: &DaemonConfig,
    ) -> Result<Self, String> {
        let tray_events = events.clone();
        let (tray, shortcut) = super::start_parts(
            || tray::TrayGuard::start(tray_events),
            || {
                shortcut::ShortcutGuard::start(
                    events,
                    &config.capture_region_hotkey,
                    #[cfg(feature = "ocr")]
                    config.capture_text_hotkey.as_ref(),
                )
            },
        )?;
        Ok(Self {
            _shortcut: shortcut,
            _tray: tray,
        })
    }
}
