mod commands;
#[cfg(test)]
mod css_token_sync;
mod launch;
mod native_capture;
#[cfg(not(target_os = "linux"))]
mod overlay;
mod scroll;
mod session;
#[cfg(target_os = "linux")]
mod webkit_workaround;

use std::sync::Arc;

use launch::LaunchMode;
use session::SharedSession;
#[cfg(not(target_os = "linux"))]
fn setup_host_window(app: &mut tauri::App, shared_session: &Arc<SharedSession>) {
    use tauri::Manager;
    if let Some(window) = app.get_webview_window("main") {
        let overlay_exclusion = overlay::configure_overlay_window(&window);
        shared_session.set_overlay_exclusion(overlay_exclusion);
    }
}

#[cfg(target_os = "linux")]
fn setup_host_window(_app: &mut tauri::App, _shared_session: &Arc<SharedSession>) {
    // R2: the native layer-shell overlay (run_native_capture) owns capture
    // input via an exclusive-keyboard layer surface. The host window must stay
    // hidden/unfocused so it cannot steal that focus (KWin would, per the Phase
    // 2/3 spike). The webview is still created (tauri.conf.json visible:false)
    // so its GPU context stays alive for wgpu/webkit coexistence (R1); we simply
    // never show or focus it.
}

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

    #[cfg(target_os = "linux")]
    webkit_workaround::apply();

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(launch_options)
        .manage(Arc::clone(&shared_session))
        .setup({
            let shared_session = Arc::clone(&shared_session);
            move |app| {
                setup_host_window(app, &shared_session);
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
            native_capture::exit_app,
            native_capture::run_native_capture,
            native_capture::uses_native_overlay,
            scroll::set_input_passthrough,
        ])
        .run(tauri::generate_context!())
        .expect("error while running rollshot app");
}
