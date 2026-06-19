use crate::daemon::core::DaemonEvent;

pub(crate) const CAPTURE_ID: &str = "capture-region";
pub(crate) const QUIT_ID: &str = "quit-rollshot";

/// Map a tray menu item id to the daemon semantic event it triggers. Unknown
/// ids are ignored so a stray menu event can never drive product behavior.
pub(crate) fn daemon_event_for(id: &str) -> Option<DaemonEvent> {
    match id {
        CAPTURE_ID => Some(DaemonEvent::CaptureRegion),
        QUIT_ID => Some(DaemonEvent::Quit),
        _ => None,
    }
}

use tray_icon::menu::{Menu, MenuEvent, MenuId, MenuItem};
use tray_icon::{TrayIcon, TrayIconBuilder};
use winit::event_loop::EventLoopProxy;

/// Owns the macOS status item and its menu for the daemon lifetime. The menu
/// exposes exactly the two product actions. `MenuEvent` is a process-global
/// handler, so `Drop` clears it to avoid a stale closure outliving the daemon.
pub(crate) struct TrayGuard {
    _tray: TrayIcon,
}

impl TrayGuard {
    pub(crate) fn start(proxy: EventLoopProxy<DaemonEvent>) -> Result<Self, String> {
        let menu = Menu::new();
        let capture = MenuItem::with_id(MenuId::new(CAPTURE_ID), "Capture Region", true, None);
        let quit = MenuItem::with_id(MenuId::new(QUIT_ID), "Quit Rollshot", true, None);
        menu.append(&capture)
            .map_err(|error| format!("failed to build tray menu: {error}"))?;
        menu.append(&quit)
            .map_err(|error| format!("failed to build tray menu: {error}"))?;

        MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
            if let Some(daemon_event) = daemon_event_for(event.id.as_ref()) {
                let _ = proxy.send_event(daemon_event);
            }
        }));

        // Title-only status item: no embedded icon asset is required and the
        // item stays visible in the menu bar on macOS.
        let tray = TrayIconBuilder::new()
            .with_menu(Box::new(menu))
            .with_title("Rollshot")
            .with_tooltip("Rollshot")
            .build()
            .map_err(|error| format!("failed to create macOS tray icon: {error}"))?;

        Ok(Self { _tray: tray })
    }
}

impl Drop for TrayGuard {
    fn drop(&mut self) {
        MenuEvent::set_event_handler(None::<fn(MenuEvent)>);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn menu_ids_map_to_daemon_events() {
        assert!(matches!(
            daemon_event_for(CAPTURE_ID),
            Some(DaemonEvent::CaptureRegion)
        ));
        assert!(matches!(daemon_event_for(QUIT_ID), Some(DaemonEvent::Quit)));
        assert!(daemon_event_for("unknown").is_none());
    }
}
