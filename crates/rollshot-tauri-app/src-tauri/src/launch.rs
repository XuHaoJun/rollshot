use rollshot_capture::InteractiveLaunchOptions;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LaunchMode {
    Capture(InteractiveLaunchOptions),
}

pub fn parse_launch_args<I, S>(args: I) -> Result<LaunchMode, String>
where
    I: IntoIterator<Item = S>,
    S: Into<String>,
{
    let mut args = args.into_iter().map(Into::into);
    let _program = args.next();

    let Some(flag) = args.next() else {
        return Ok(LaunchMode::Capture(
            InteractiveLaunchOptions::default_capture(),
        ));
    };

    if flag != "--capture" {
        return Err(format!("unknown rollshot-tauri-app argument '{flag}'"));
    }

    let Some(payload) = args.next() else {
        return Err("--capture requires a JSON payload".to_string());
    };

    if let Some(extra) = args.next() {
        return Err(format!(
            "unexpected argument after capture payload: '{extra}'"
        ));
    }

    let options: InteractiveLaunchOptions = serde_json::from_str(&payload)
        .map_err(|err| format!("invalid --capture JSON payload: {err}"))?;
    Ok(LaunchMode::Capture(options))
}

#[cfg(test)]
mod tests {
    use super::{parse_launch_args, LaunchMode};

    #[test]
    fn parses_capture_launch_options() {
        let mode = parse_launch_args([
            "rollshot-tauri-app",
            "--capture",
            r#"{"backend":"linux-portal","fps":7,"show_cursor":true}"#,
        ])
        .expect("parse launch args");

        match mode {
            LaunchMode::Capture(options) => {
                assert_eq!(options.backend, "linux-portal");
                assert_eq!(options.fps, 7);
                assert!(options.show_cursor);
            }
        }
    }

    #[test]
    fn no_args_uses_defaults() {
        let mode = parse_launch_args(["rollshot-tauri-app"]).expect("no args should succeed");
        match mode {
            LaunchMode::Capture(options) => {
                assert_eq!(options.backend, "auto");
                assert_eq!(options.fps, 5);
                assert!(!options.show_cursor);
            }
        }
    }

    #[test]
    fn ignores_obsolete_overlay_mode() {
        let obsolete_field = concat!("overlay", "_mode");
        let payload = format!(
            r#"{{"backend":"macos-sck","fps":30,"show_cursor":false,"{obsolete_field}":"legacy"}}"#
        );
        let mode = parse_launch_args([
            "rollshot-tauri-app",
            "--capture",
            payload.as_str(),
        ])
        .expect("parse launch args");

        match mode {
            LaunchMode::Capture(options) => {
                assert_eq!(options.backend, "macos-sck");
            }
        }
    }

    #[test]
    fn rejects_missing_capture_payload() {
        let err =
            parse_launch_args(["rollshot-tauri-app", "--capture"]).expect_err("missing payload");
        assert!(
            err.contains("--capture requires a JSON payload"),
            "err = {err}"
        );
    }

    #[test]
    fn rejects_unknown_args() {
        let err = parse_launch_args(["rollshot-tauri-app", "--bogus"]).expect_err("unknown arg");
        assert!(
            err.contains("unknown rollshot-tauri-app argument"),
            "err = {err}"
        );
    }
}
