mod commands;
mod launch;
mod overlay;
mod session;

use std::sync::Arc;

use launch::LaunchMode;
use session::SharedSession;
use tauri::Manager;

pub fn run() {
    let launch_mode = match launch::parse_launch_args(std::env::args()) {
        Ok(mode) => mode,
        Err(message) => {
            eprintln!("{message}");
            std::process::exit(1);
        }
    };

    let LaunchMode::Capture(launch_options) = launch_mode;
    let shared_session = Arc::new(SharedSession::new());

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(launch_options)
        .manage(Arc::clone(&shared_session))
        .setup({
            let shared_session = Arc::clone(&shared_session);
            move |app| {
                if let Some(window) = app.get_webview_window("main") {
                    let overlay_exclusion = overlay::configure_overlay_window(&window);
                    shared_session.set_overlay_exclusion(overlay_exclusion);
                }
                Ok(())
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::launch_options,
            commands::start_capture,
            commands::stop_capture,
            commands::session_status,
            commands::confirm_region,
            commands::get_latest_preview,
            commands::start_stitching,
            commands::stop_stitching,
            commands::save_image,
            commands::get_stitch_preview,
            commands::get_final_preview,
            commands::overlay_exclusion,
        ])
        .run(tauri::generate_context!())
        .expect("error while running rollshot app");
}
