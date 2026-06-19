pub(crate) mod shortcut;
pub(crate) mod tray;

use crate::daemon::config::DaemonConfig;
use crate::daemon::core::DaemonEvent;

fn start_parts<T, S>(
    start_tray: impl FnOnce() -> Result<T, String>,
    start_shortcut: impl FnOnce() -> Result<S, String>,
) -> Result<(T, Option<S>), String> {
    let tray = start_tray()?;
    let shortcut = match start_shortcut() {
        Ok(shortcut) => Some(shortcut),
        Err(error) => {
            tracing::warn!(
                target: "rollshot::daemon::shortcut",
                %error,
                "global shortcut unavailable; continuing with tray only"
            );
            None
        }
    };
    Ok((tray, shortcut))
}

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
        let (tray, shortcut) = start_parts(
            || tray::TrayGuard::start(tray_events),
            || shortcut::ShortcutGuard::start(events, &config.capture_region_hotkey),
        )?;
        Ok(Self {
            _shortcut: shortcut,
            _tray: tray,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tray_failure_aborts_platform_startup() {
        assert!(start_parts::<(), ()>(|| Err("no tray".into()), || Ok(())).is_err());
    }

    #[test]
    fn shortcut_worker_start_failure_keeps_tray_alive() {
        let (tray, shortcut) = start_parts(|| Ok(7), || Err::<(), _>("denied".into())).unwrap();
        assert_eq!(tray, 7);
        assert!(shortcut.is_none());
    }
}
