mod launch;

use launch::LaunchMode;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PostOverlayAction {
    ExitSuccess,
    ExitCancelled,
}

pub fn post_overlay_action(
    result: Result<Option<rollshot_iced_overlay::CaptureResult>, String>,
) -> PostOverlayAction {
    match result {
        Ok(Some(_)) => PostOverlayAction::ExitSuccess,
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

    let result = rollshot_iced_overlay::run_overlay(config);
    match post_overlay_action(result.map_err(|e| e.to_string())) {
        PostOverlayAction::ExitSuccess => println!("capture complete"),
        PostOverlayAction::ExitCancelled => println!("capture cancelled"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn capture_result() -> rollshot_iced_overlay::CaptureResult {
        rollshot_iced_overlay::CaptureResult {
            image: image::RgbaImage::new(1, 1),
            stats: None,
            post_overlay_request: rollshot_iced_overlay::PostOverlayRequest::None,
        }
    }

    #[test]
    fn completed_overlay_does_not_open_another_save_dialog() {
        assert_eq!(
            post_overlay_action(Ok(Some(capture_result()))),
            PostOverlayAction::ExitSuccess
        );
    }

    #[test]
    fn cancelled_overlay_exits_successfully() {
        assert_eq!(
            post_overlay_action(Ok(None)),
            PostOverlayAction::ExitCancelled
        );
    }

    #[test]
    fn save_dialog_temp_mode_is_no_longer_accepted() {
        assert!(
            launch::parse_launch_args(["rollshot-app", "--save-dialog-temp", "/tmp/a.png"])
                .is_err()
        );
    }
}
