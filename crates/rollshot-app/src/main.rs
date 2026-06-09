mod launch;

use launch::LaunchMode;

mod save;
mod storage;

#[derive(Debug, Clone)]
pub enum PostOverlayAction {
    ExitSuccess,
    ExitCancelled,
    SaveAs(rollshot_iced_overlay::CaptureResult),
}

pub fn post_overlay_action(
    result: Result<Option<rollshot_iced_overlay::CaptureResult>, String>,
) -> PostOverlayAction {
    match result {
        // The overlay is now capture-only and never requests a post-overlay save
        // dialog; a completed capture always means success. Task 6 removes the
        // remaining PostOverlayRequest/SaveAs plumbing.
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

    let result = rollshot_iced_overlay::run_overlay(config);
    match post_overlay_action(result.map_err(|e| e.to_string())) {
        PostOverlayAction::ExitSuccess => println!("capture complete"),
        PostOverlayAction::ExitCancelled => println!("capture cancelled"),
        PostOverlayAction::SaveAs(cr) => {
            if let Err(e) = save::save_as(&cr.image) {
                eprintln!("{e}");
                std::process::exit(1);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn capture_result() -> rollshot_iced_overlay::CaptureResult {
        rollshot_iced_overlay::CaptureResult {
            image: image::RgbaImage::new(1, 1),
            stats: None,
        }
    }

    #[test]
    fn completed_overlay_does_not_open_another_save_dialog() {
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

    #[test]
    fn save_dialog_temp_mode_is_no_longer_accepted() {
        assert!(
            launch::parse_launch_args(["rollshot-app", "--save-dialog-temp", "/tmp/a.png"])
                .is_err()
        );
    }
}
