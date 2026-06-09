mod launch;

use launch::LaunchMode;

// Registered on every target so the portable thumbnail timer + interaction
// helpers compile and unit-test on Linux. Only its `view` is macOS-gated.
#[cfg(target_os = "macos")]
mod macos_product;
mod macos_thumbnail;
mod post_capture;
mod result_workspace;
mod storage;

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
    }
}

fn run_iced_capture(options: rollshot_capture::InteractiveLaunchOptions) {
    let config = rollshot_iced_overlay::OverlayConfig {
        backend: options.backend,
        fps: options.fps,
        show_cursor: options.show_cursor,
        initial_mode: options.initial_mode,
    };

    #[cfg(target_os = "linux")]
    {
        if let Err(e) = run_product_capture(config) {
            eprintln!("{e}");
            std::process::exit(1);
        }
    }

    #[cfg(target_os = "macos")]
    {
        if let Err(e) = run_product_capture(config) {
            eprintln!("{e}");
            std::process::exit(1);
        }
    }

    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        let _ = config;
        eprintln!("unsupported platform");
        std::process::exit(1);
    }
}

#[cfg(target_os = "linux")]
fn run_product_capture(config: rollshot_iced_overlay::OverlayConfig) -> Result<(), String> {
    match post_capture::capture_completion(
        rollshot_iced_overlay::run_overlay(config).map_err(|e| e.to_string())?,
    ) {
        post_capture::CaptureCompletion::Present(result) => {
            post_capture::handle_linux_capture(result)
        }
        post_capture::CaptureCompletion::Cancelled => Ok(()),
    }
}

/// macOS: route capture into the single-process product daemon, which owns the
/// whole capture → thumbnail / Result Workspace flow in one event loop.
#[cfg(target_os = "macos")]
fn run_product_capture(config: rollshot_iced_overlay::OverlayConfig) -> Result<(), String> {
    macos_product::run(config)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn save_dialog_temp_mode_is_no_longer_accepted() {
        assert!(
            launch::parse_launch_args(["rollshot-app", "--save-dialog-temp", "/tmp/a.png"])
                .is_err()
        );
    }
}
