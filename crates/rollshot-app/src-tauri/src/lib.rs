mod commands;
mod launch;
mod session;

use std::sync::Arc;

use launch::LaunchMode;
use session::SharedSession;

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
        .manage(launch_options)
        .manage(Arc::clone(&shared_session))
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
            commands::get_final_preview
        ])
        .run(tauri::generate_context!())
        .expect("error while running rollshot app");
}
