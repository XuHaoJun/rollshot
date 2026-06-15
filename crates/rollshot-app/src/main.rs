mod diagnostics;
mod launch;

use launch::LaunchMode;
use std::process::ExitCode;

// Registered on every target so the portable thumbnail timer + interaction
// helpers compile and unit-test on Linux. Only its `view` is macOS-gated.
#[cfg(target_os = "macos")]
mod macos_product;
// Registered on every target so the pure drag placement/result helpers compile
// and unit-test on Linux; the AppKit bridge inside it is macOS-gated.
mod macos_native_drag;
mod macos_thumbnail;
mod post_capture;
mod result_workspace;
mod storage;

fn main() -> ExitCode {
    let logging = match launch::extract_logging_args(std::env::args()) {
        Ok(logging) => logging,
        Err(message) => {
            eprintln!("{message}");
            return ExitCode::FAILURE;
        }
    };

    let selected = diagnostics::select_filter(std::env::var("RUST_LOG").ok().as_deref());
    let _diagnostics = match diagnostics::init(logging.log_file.as_deref(), &selected) {
        Ok(guard) => guard,
        Err(message) => {
            eprintln!("{message}");
            return ExitCode::FAILURE;
        }
    };

    if !selected.ignored.is_empty() {
        tracing::warn!(
            target: diagnostics::TARGET_FILTER,
            ignored = ?selected.ignored,
            "ignored invalid RUST_LOG directives"
        );
    }

    match run(logging.remaining, logging.log_file.is_some()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            tracing::error!(
                target: diagnostics::TARGET_APP,
                error_category = diagnostics::classify_app_error(&error),
                "application failed"
            );
            ExitCode::FAILURE
        }
    }
}

fn run(args: Vec<String>, file_logging: bool) -> Result<(), String> {
    let launch_mode = launch::parse_launch_args(args)?;

    match launch_mode {
        LaunchMode::Capture(options) => {
            tracing::info!(
                target: diagnostics::TARGET_APP,
                version = env!("CARGO_PKG_VERSION"),
                os = std::env::consts::OS,
                arch = std::env::consts::ARCH,
                backend = options.backend.as_str(),
                fps = options.fps,
                show_cursor = options.show_cursor,
                initial_mode = ?options.initial_mode,
                file_logging,
                "capture session started"
            );
            run_iced_capture(options)
        }
    }
}

fn run_iced_capture(options: rollshot_capture::InteractiveLaunchOptions) -> Result<(), String> {
    let config = rollshot_iced_overlay::OverlayConfig {
        backend: options.backend,
        fps: options.fps,
        show_cursor: options.show_cursor,
        request: options.initial_mode.into(),
        target_output_name: None,
    };

    #[cfg(target_os = "linux")]
    {
        run_product_capture(config)
    }

    #[cfg(target_os = "macos")]
    {
        run_product_capture(config)
    }

    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        let _ = config;
        Err("unsupported platform".to_string())
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

    #[test]
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    fn run_returns_error_for_unsupported_platform() {
        let err = run(
            vec![
                "rollshot-app".to_string(),
                "--capture".to_string(),
                r#"{"backend":"auto","fps":5,"show_cursor":false}"#.to_string(),
            ],
            false,
        );
        assert!(err.is_err());
    }
}
