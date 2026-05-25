use std::time::Duration;

use enigo::{Axis, Enigo, Mouse, Settings};
use tokio::sync::Mutex;
use tokio::time::sleep;

pub struct EnigoState(Mutex<Option<Enigo>>);

impl EnigoState {
    pub fn new() -> Self {
        Self(Mutex::new(None))
    }
}

#[tauri::command]
pub async fn scroll_through(
    window: tauri::Window,
    enigo_state: tauri::State<'_, EnigoState>,
    length: i32,
) -> Result<(), String> {
    let mut guard = enigo_state.0.lock().await;
    if guard.is_none() {
        let enigo = Enigo::new(&Settings::default())
            .map_err(|err| format!("failed to init enigo: {err}"))?;
        *guard = Some(enigo);
    }

    window
        .set_ignore_cursor_events(true)
        .map_err(|err| format!("failed to enable ignore cursor events: {err}"))?;
    sleep(Duration::from_millis(5)).await;

    if let Some(enigo) = guard.as_mut() {
        if let Err(err) = enigo.scroll(length, Axis::Vertical) {
            eprintln!("scroll_through: enigo scroll failed: {err}");
        }
    }

    sleep(Duration::from_millis(32)).await;
    let _ = window.set_ignore_cursor_events(false);
    Ok(())
}
