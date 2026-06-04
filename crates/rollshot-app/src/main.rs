mod launch;
mod save;

use launch::LaunchMode;

fn main() {
    let launch_mode = match launch::parse_launch_args(std::env::args()) {
        Ok(mode) => mode,
        Err(message) => {
            eprintln!("{message}");
            std::process::exit(1);
        }
    };

    match launch_mode {
        LaunchMode::Capture(options) => {
            run_iced_capture(options);
        }
        LaunchMode::SaveDialogTemp(path) => {
            if let Err(err) = run_save_dialog_helper(&path) {
                eprintln!("{err}");
                std::process::exit(1);
            }
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

/// Save after the stitched result exists. The save dialog runs in a helper
/// process so AppKit/rfd does not share state with the completed iced/winit
/// event loop.
fn handle_capture_result(result: rollshot_iced_overlay::CaptureResult) {
    if let Err(err) = save_result_via_helper(&result) {
        eprintln!("{err}");
        std::process::exit(1);
    }
}

fn save_result_via_helper(result: &rollshot_iced_overlay::CaptureResult) -> Result<(), String> {
    let temp_path = temp_capture_path();
    let save_result = save::write_png(&result.image, &temp_path).and_then(|()| {
        let exe = std::env::current_exe()
            .map_err(|err| format!("failed to resolve rollshot-app executable: {err}"))?;
        let status = std::process::Command::new(exe)
            .arg("--save-dialog-temp")
            .arg(&temp_path)
            .status()
            .map_err(|err| format!("failed to launch save dialog helper: {err}"))?;
        if status.success() {
            Ok(())
        } else {
            Err(format!("save dialog helper exited with status {status}"))
        }
    });
    std::fs::remove_file(&temp_path).ok();
    save_result
}

fn run_save_dialog_helper(source: &std::path::Path) -> Result<(), String> {
    match save::prompt_save_path() {
        Some(destination) => {
            std::fs::copy(source, &destination).map_err(|err| {
                format!(
                    "failed to write PNG to {} from {}: {err}",
                    destination.display(),
                    source.display()
                )
            })?;
            println!("saved {}", destination.display());
        }
        None => println!("save cancelled (capture not written)"),
    }
    Ok(())
}

fn temp_capture_path() -> std::path::PathBuf {
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    std::env::temp_dir().join(format!(
        "rollshot-capture-{}-{unique}.png",
        std::process::id()
    ))
}
