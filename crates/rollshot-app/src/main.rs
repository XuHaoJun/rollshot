mod launch;

use launch::LaunchMode;

mod post_capture;
mod result_workspace;
mod storage;

#[cfg(target_os = "macos")]
#[derive(Debug, Clone)]
pub enum PostOverlayAction {
    ExitSuccess,
    ExitCancelled,
}

#[cfg(target_os = "macos")]
pub fn post_overlay_action(
    result: Result<Option<rollshot_iced_overlay::CaptureResult>, String>,
) -> PostOverlayAction {
    match result {
        Ok(Some(_cr)) => PostOverlayAction::ExitSuccess,
        Ok(None) => PostOverlayAction::ExitCancelled,
        Err(err) => {
            eprintln!("{err}");
            std::process::exit(1);
        }
    }
}

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
        let result = rollshot_iced_overlay::run_overlay(config);
        match post_overlay_action(result.map_err(|e| e.to_string())) {
            PostOverlayAction::ExitSuccess => println!("capture complete"),
            PostOverlayAction::ExitCancelled => println!("capture cancelled"),
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

    #[cfg(target_os = "macos")]
    mod macos_tests {
        use super::*;

        fn capture_result() -> rollshot_iced_overlay::CaptureResult {
            rollshot_iced_overlay::CaptureResult {
                image: image::RgbaImage::new(1, 1),
                stats: None,
            }
        }

        #[test]
        fn completed_overlay_exits_with_success() {
            assert!(matches!(
                post_overlay_action(Ok(Some(capture_result()))),
                PostOverlayAction::ExitSuccess
            ));
        }

        #[test]
        fn cancelled_overlay_exits_successfully() {
            assert!(matches!(
                post_overlay_action(Ok(None)),
                PostOverlayAction::ExitCancelled
            ));
        }
    }
}
