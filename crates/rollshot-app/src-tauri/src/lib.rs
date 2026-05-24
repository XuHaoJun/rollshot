mod launch;

use launch::LaunchMode;

pub fn run() {
    let launch_mode = match launch::parse_launch_args(std::env::args()) {
        Ok(mode) => mode,
        Err(message) => {
            eprintln!("{message}");
            std::process::exit(1);
        }
    };

    let _launch_options = match launch_mode {
        LaunchMode::Capture(options) => options,
    };

    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![])
        .run(tauri::generate_context!())
        .expect("error while running rollshot app");
}
