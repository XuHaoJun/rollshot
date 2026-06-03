#[tauri::command]
pub async fn set_input_passthrough(window: tauri::Window, enabled: bool) -> Result<(), String> {
    window
        .set_ignore_cursor_events(enabled)
        .map_err(|err| format!("failed to set input passthrough: {err}"))
}
