use crate::daemon::core::DaemonEvent;
use std::sync::mpsc::Sender;

pub struct DaemonTrayItem {
    events: Sender<DaemonEvent>,
}

impl DaemonTrayItem {
    pub(crate) fn new(events: Sender<DaemonEvent>) -> Self {
        Self { events }
    }

    fn activate_capture(&mut self) {
        let _ = self.events.send(DaemonEvent::CaptureRegion);
    }

    fn activate_quit(&mut self) {
        let _ = self.events.send(DaemonEvent::Quit);
    }
}

impl ksni::Tray for DaemonTrayItem {
    fn id(&self) -> String {
        "rollshot-daemon".into()
    }

    fn title(&self) -> String {
        "Rollshot".into()
    }

    fn icon_name(&self) -> String {
        "camera-photo".into()
    }

    fn menu(&self) -> Vec<ksni::MenuItem<Self>> {
        use ksni::menu::StandardItem;
        vec![
            StandardItem {
                label: "Capture Region".into(),
                icon_name: "camera-photo".into(),
                activate: Box::new(Self::activate_capture),
                ..Default::default()
            }
            .into(),
            StandardItem {
                label: "Quit Rollshot".into(),
                icon_name: "application-exit".into(),
                activate: Box::new(Self::activate_quit),
                ..Default::default()
            }
            .into(),
        ]
    }
}

pub struct TrayGuard {
    handle: ksni::blocking::Handle<DaemonTrayItem>,
}

impl TrayGuard {
    pub fn start(events: Sender<DaemonEvent>) -> Result<Self, String> {
        if !rollshot_linux_desktop::sni_host_available() {
            return Err("KDE StatusNotifierHost is unavailable".into());
        }
        use ksni::blocking::TrayMethods;
        let handle = DaemonTrayItem::new(events)
            .spawn()
            .map_err(|error| format!("failed to register Rollshot tray: {error}"))?;
        Ok(Self { handle })
    }
}

impl Drop for TrayGuard {
    fn drop(&mut self) {
        self.handle.shutdown().wait();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capture_menu_item_sends_capture_event() {
        let (tx, rx) = std::sync::mpsc::channel();
        let mut item = DaemonTrayItem::new(tx);
        item.activate_capture();
        assert!(matches!(rx.recv().unwrap(), DaemonEvent::CaptureRegion));
    }

    #[test]
    fn quit_menu_item_sends_quit_event() {
        let (tx, rx) = std::sync::mpsc::channel();
        let mut item = DaemonTrayItem::new(tx);
        item.activate_quit();
        assert!(matches!(rx.recv().unwrap(), DaemonEvent::Quit));
    }

    #[test]
    fn menu_contains_only_capture_and_quit() {
        let (tx, _rx) = std::sync::mpsc::channel();
        let item = DaemonTrayItem::new(tx);
        let menu = ksni::Tray::menu(&item);
        let labels: Vec<&str> = menu
            .iter()
            .map(|item| match item {
                ksni::MenuItem::Standard(item) => item.label.as_str(),
                _ => panic!("daemon tray only uses standard items"),
            })
            .collect();
        assert_eq!(labels, ["Capture Region", "Quit Rollshot"]);
    }
}
