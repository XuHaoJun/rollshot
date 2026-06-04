mod launch;
mod overlay_selection;
mod save;

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
        Ok(Some(result)) => handle_capture_result(result),
        Ok(None) => {
            println!("capture cancelled");
        }
        Err(err) => {
            eprintln!("{err}");
            std::process::exit(1);
        }
    }
}

/// Prompt for a destination with a native save dialog (Option A) and write the
/// stitched PNG, mirroring the Tauri app's Esc → save-prompt flow. Runs on the
/// main thread after the iced event loop has exited, on both macOS and Linux.
fn handle_capture_result(result: rollshot_iced_overlay::CaptureResult) {
    match save::prompt_save_path() {
        Some(path) => match save::write_png(&result.image, &path) {
            Ok(()) => println!("saved {}", path.display()),
            Err(err) => {
                eprintln!("{err}");
                std::process::exit(1);
            }
        },
        None => println!(
            "save cancelled ({}x{} captured, not written)",
            result.image.width(),
            result.image.height()
        ),
    }
}
