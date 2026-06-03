mod launch;
mod overlay_selection;

use launch::LaunchMode;
use overlay_selection::{resolve_overlay_runner, OverlayRunner};

fn main() {
    let launch_mode = match launch::parse_launch_args(std::env::args()) {
        Ok(mode) => mode,
        Err(message) => {
            eprintln!("{message}");
            std::process::exit(1);
        }
    };

    let LaunchMode::Capture(options) = launch_mode;
    match resolve_overlay_runner(std::env::consts::OS, options.overlay_mode) {
        OverlayRunner::Iced => run_iced_capture(options),
        OverlayRunner::Tauri => {
            eprintln!(
                "selected overlay mode resolves to the retained Tauri overlay; run rollshot-tauri-app or pass overlay_mode=\"iced\" for the iced validation path"
            );
            std::process::exit(2);
        }
    }
}

fn run_iced_capture(options: rollshot_capture::InteractiveLaunchOptions) {
    let config = rollshot_iced_overlay::OverlayConfig {
        backend: options.backend,
        fps: options.fps,
        show_cursor: options.show_cursor,
    };

    match rollshot_iced_overlay::run_overlay(config) {
        Ok(Some(result)) => {
            println!(
                "captured {}x{} ({} frames)",
                result.image.width(),
                result.image.height(),
                result.stats.frame_count
            );
        }
        Ok(None) => {
            println!("capture cancelled");
        }
        Err(err) => {
            eprintln!("{err}");
            std::process::exit(1);
        }
    }
}
