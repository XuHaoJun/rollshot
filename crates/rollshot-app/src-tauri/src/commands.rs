use std::sync::Arc;

use rollshot_capture::InteractiveLaunchOptions;
use tauri::ipc::Response;

use crate::session::{RegionDto, SessionStatus, SharedSession};

#[tauri::command]
pub fn launch_options(
    options: tauri::State<'_, InteractiveLaunchOptions>,
) -> InteractiveLaunchOptions {
    options.inner().clone()
}

#[tauri::command]
pub fn start_capture(
    session: tauri::State<'_, Arc<SharedSession>>,
    options: InteractiveLaunchOptions,
) -> Result<(), String> {
    session.start_capture(options)
}

#[tauri::command]
pub fn stop_capture(session: tauri::State<'_, Arc<SharedSession>>) -> Result<(), String> {
    session.stop_capture();
    Ok(())
}

#[tauri::command]
pub fn session_status(
    session: tauri::State<'_, Arc<SharedSession>>,
) -> Result<SessionStatus, String> {
    session.status()
}

#[tauri::command]
pub fn confirm_region(
    session: tauri::State<'_, Arc<SharedSession>>,
    region: RegionDto,
) -> Result<RegionDto, String> {
    session.confirm_region(region)
}

#[tauri::command]
pub fn get_latest_preview(
    session: tauri::State<'_, Arc<SharedSession>>,
    max_edge: u32,
) -> Result<Response, String> {
    let bytes = session.latest_preview_png(max_edge)?.unwrap_or_default();
    Ok(Response::new(bytes))
}
