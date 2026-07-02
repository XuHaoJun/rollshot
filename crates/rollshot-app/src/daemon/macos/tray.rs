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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct TrayVisualConfig {
    uses_icon: bool,
    icon_is_template: bool,
    uses_title: bool,
}

fn normal_tray_visual_config() -> TrayVisualConfig {
    TrayVisualConfig {
        uses_icon: true,
        icon_is_template: true,
        uses_title: false,
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

        let icon = crate::daemon::tray_icon::normal_tray_icon()?;
        let visual = normal_tray_visual_config();
        let mut builder = TrayIconBuilder::new()
            .with_menu(Box::new(menu))
            .with_tooltip("Rollshot");
        if visual.uses_icon {
            builder = builder.with_icon(icon);
        }
        if visual.icon_is_template {
            builder = builder.with_icon_as_template(true);
        }
        if visual.uses_title {
            builder = builder.with_title("Rollshot");
        }
        let tray = builder
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

    #[test]
    fn normal_tray_uses_template_icon_without_title() {
        let config = normal_tray_visual_config();
        assert!(config.uses_icon);
        assert!(config.icon_is_template);
        assert!(!config.uses_title);
    }
}
