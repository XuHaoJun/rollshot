use crate::daemon::core::DaemonEvent;
use std::sync::mpsc::Sender;

pub struct DaemonTrayItem {
    events: Sender<DaemonEvent>,
    icon: ksni::Icon,
}

impl DaemonTrayItem {
    pub(crate) fn new(events: Sender<DaemonEvent>, icon: ksni::Icon) -> Self {
        Self { events, icon }
    }

    fn activate_capture(&mut self) {
        let _ = self.events.send(DaemonEvent::CaptureRegion);
    }

    #[cfg(feature = "ocr")]
    fn activate_text(&mut self) {
        let _ = self.events.send(DaemonEvent::CaptureText);
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
        "rollshot".into()
    }

    fn icon_pixmap(&self) -> Vec<ksni::Icon> {
        vec![self.icon.clone()]
    }

    fn menu(&self) -> Vec<ksni::MenuItem<Self>> {
        use ksni::menu::StandardItem;
        let mut items = vec![StandardItem {
            label: "Capture Region".into(),
            icon_name: "camera-photo".into(),
            activate: Box::new(Self::activate_capture),
            ..Default::default()
        }
        .into()];
        #[cfg(feature = "ocr")]
        {
            items.push(
                StandardItem {
                    label: "Capture Text".into(),
                    icon_name: "text-x-generic".into(),
                    activate: Box::new(Self::activate_text),
                    ..Default::default()
                }
                .into(),
            );
        }
        items.push(
            StandardItem {
                label: "Quit Rollshot".into(),
                icon_name: "application-exit".into(),
                activate: Box::new(Self::activate_quit),
                ..Default::default()
            }
            .into(),
        );
        items
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
        let icon = crate::daemon::tray_icon::normal_ksni_icon()?;
        use ksni::blocking::TrayMethods;
        let handle = DaemonTrayItem::new(events, icon)
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

    fn test_icon() -> ksni::Icon {
        ksni::Icon {
            width: 1,
            height: 1,
            data: vec![255, 0, 0, 0],
        }
    }

    #[test]
    fn capture_menu_item_sends_capture_event() {
        let (tx, rx) = std::sync::mpsc::channel();
        let mut item = DaemonTrayItem::new(tx, test_icon());
        item.activate_capture();
        assert!(matches!(rx.recv().unwrap(), DaemonEvent::CaptureRegion));
    }

    #[test]
    fn quit_menu_item_sends_quit_event() {
        let (tx, rx) = std::sync::mpsc::channel();
        let mut item = DaemonTrayItem::new(tx, test_icon());
        item.activate_quit();
        assert!(matches!(rx.recv().unwrap(), DaemonEvent::Quit));
    }

    #[cfg(not(feature = "ocr"))]
    #[test]
    fn menu_contains_only_capture_and_quit() {
        let (tx, _rx) = std::sync::mpsc::channel();
        let item = DaemonTrayItem::new(tx, test_icon());
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

    #[test]
    fn tray_exposes_embedded_icon_pixmap() {
        let (tx, _rx) = std::sync::mpsc::channel();
        let item = DaemonTrayItem::new(tx, test_icon());
        let pixmaps = ksni::Tray::icon_pixmap(&item);
        assert_eq!(pixmaps.len(), 1);
        assert_eq!(pixmaps[0].width, 1);
        assert_eq!(pixmaps[0].height, 1);
        assert_eq!(pixmaps[0].data, [255, 0, 0, 0]);
    }

    #[cfg(feature = "ocr")]
    #[test]
    fn ocr_menu_contains_region_text_and_quit() {
        let (tx, _rx) = std::sync::mpsc::channel();
        let item = DaemonTrayItem::new(tx, test_icon());
        let menu = ksni::Tray::menu(&item);
        let labels: Vec<&str> = menu
            .iter()
            .map(|item| match item {
                ksni::MenuItem::Standard(item) => item.label.as_str(),
                _ => panic!("daemon tray only uses standard items"),
            })
            .collect();
        assert_eq!(labels, ["Capture Region", "Capture Text", "Quit Rollshot"]);
    }

    #[cfg(feature = "ocr")]
    #[test]
    fn activate_text_sends_capture_text_event() {
        let (tx, rx) = std::sync::mpsc::channel();
        let mut item = DaemonTrayItem::new(tx, test_icon());
        item.activate_text();
        assert!(matches!(rx.recv().unwrap(), DaemonEvent::CaptureText));
    }
}
