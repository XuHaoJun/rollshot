use iced::futures::channel::mpsc::{self, UnboundedReceiver};
use iced::futures::StreamExt;
use std::sync::Mutex;

use crate::macos_recording_status::{
    recording_status, status_title, status_tooltip, RecordingStatus,
};

use tray_icon::menu::{Menu, MenuEvent, MenuId, MenuItem};
use tray_icon::{TrayIcon, TrayIconBuilder};

const FINISH_ID: &str = "finish-action-guide-recording";
const CANCEL_ID: &str = "cancel-action-guide-recording";

static EVENT_RX: Mutex<Option<UnboundedReceiver<Event>>> = Mutex::new(None);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Event {
    Finish,
    Cancel,
}

fn event_for(id: &str) -> Option<Event> {
    match id {
        FINISH_ID => Some(Event::Finish),
        CANCEL_ID => Some(Event::Cancel),
        _ => None,
    }
}

pub struct Guard {
    tray: TrayIcon,
    status: RecordingStatus,
}

impl Guard {
    pub fn start() -> Result<Self, String> {
        let menu = Menu::new();
        let finish = MenuItem::with_id(MenuId::new(FINISH_ID), "Finish Recording", true, None);
        let cancel = MenuItem::with_id(MenuId::new(CANCEL_ID), "Cancel Recording", true, None);
        menu.append(&finish)
            .map_err(|error| format!("failed to build recording menu: {error}"))?;
        menu.append(&cancel)
            .map_err(|error| format!("failed to build recording menu: {error}"))?;

        let (tx, rx) = mpsc::unbounded();
        *EVENT_RX.lock().unwrap() = Some(rx);
        MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
            if let Some(event) = event_for(event.id.as_ref()) {
                let _ = tx.unbounded_send(event);
            }
        }));

        let status = RecordingStatus::Off;
        let tray = TrayIconBuilder::new()
            .with_menu(Box::new(menu))
            .with_title(status_title(status))
            .with_tooltip(status_tooltip(status))
            .build()
            .map_err(|error| format!("failed to create recording tray: {error}"))?;

        Ok(Self { tray, status })
    }

    /// Update the tray to reflect a new motion runtime status.
    pub fn set_motion_status(&mut self, status: rollshot_action::motion::MotionRuntimeStatus) {
        let new = recording_status(status, self.status);
        if new == self.status {
            return;
        }
        self.status = new;
        let _ = self.tray.set_title(Some(&status_title(new)));
        let _ = self.tray.set_tooltip(Some(&status_tooltip(new)));
    }
}

impl Drop for Guard {
    fn drop(&mut self) {
        MenuEvent::set_event_handler(None::<fn(MenuEvent)>);
        *EVENT_RX.lock().unwrap() = None;
    }
}

pub fn subscription() -> iced::Subscription<Event> {
    iced::Subscription::run(|| {
        let rx = EVENT_RX.lock().unwrap().take();
        match rx {
            Some(rx) => rx.boxed(),
            None => iced::futures::stream::pending().boxed(),
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn menu_ids_map_to_recording_events() {
        assert_eq!(event_for(FINISH_ID), Some(Event::Finish));
        assert_eq!(event_for(CANCEL_ID), Some(Event::Cancel));
        assert_eq!(event_for("unknown"), None);
    }
}
