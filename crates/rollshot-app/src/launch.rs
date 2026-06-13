use rollshot_capture::InteractiveLaunchOptions;
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LaunchMode {
    Capture(InteractiveLaunchOptions),
}

#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoggingArgs {
    pub log_file: Option<PathBuf>,
    pub remaining: Vec<String>,
}

#[allow(dead_code)]
pub fn extract_logging_args<I, S>(args: I) -> Result<LoggingArgs, String>
where
    I: IntoIterator<Item = S>,
    S: Into<String>,
{
    let mut input = args.into_iter().map(Into::into);
    let program = input.next().unwrap_or_else(|| "rollshot-app".to_string());
    let mut remaining = vec![program];
    let mut log_file = None;

    while let Some(arg) = input.next() {
        if arg == "--log-file" {
            if log_file.is_some() {
                return Err("--log-file may only be specified once".to_string());
            }
            let path = input
                .next()
                .ok_or_else(|| "--log-file requires a path".to_string())?;
            log_file = Some(PathBuf::from(path));
        } else {
            remaining.push(arg);
        }
    }

    Ok(LoggingArgs {
        log_file,
        remaining,
    })
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
        return Err(format!("unknown rollshot-app argument '{flag}'"));
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
    use super::{extract_logging_args, parse_launch_args, LaunchMode};
    use rollshot_capture::CaptureMode;
    use std::path::PathBuf;

    #[test]
    fn no_args_uses_defaults() {
        let mode = parse_launch_args(["rollshot-app"]).expect("no args should succeed");
        match mode {
            LaunchMode::Capture(options) => {
                assert_eq!(options.backend, "auto");
                assert_eq!(options.fps, 5);
                assert!(!options.show_cursor);
                assert_eq!(options.initial_mode, CaptureMode::Scrolling);
            }
        }
    }

    #[test]
    fn ignores_obsolete_capture_option() {
        let obsolete_field = concat!("overlay", "_mode");
        let payload = format!(
            r#"{{"backend":"macos-sck","fps":30,"show_cursor":false,"{obsolete_field}":"legacy"}}"#
        );
        let mode = parse_launch_args(["rollshot-app", "--capture", payload.as_str()])
            .expect("parse launch args");

        match mode {
            LaunchMode::Capture(options) => {
                assert_eq!(options.backend, "macos-sck");
                assert_eq!(options.initial_mode, CaptureMode::Scrolling);
            }
        }
    }

    #[test]
    fn save_dialog_temp_mode_is_rejected() {
        let err = parse_launch_args(["rollshot-app", "--save-dialog-temp", "/tmp/rollshot.png"])
            .expect_err("save-dialog-temp should be rejected");
        assert!(err.contains("unknown rollshot-app argument"), "err = {err}");
    }

    #[test]
    fn rejects_unknown_flag() {
        let err = parse_launch_args(["rollshot-app", "--bogus"]).expect_err("unknown arg");
        assert!(err.contains("unknown rollshot-app argument"), "err = {err}");
    }

    #[test]
    fn rejects_missing_capture_payload() {
        let err = parse_launch_args(["rollshot-app", "--capture"]).expect_err("missing payload");
        assert!(
            err.contains("--capture requires a JSON payload"),
            "err = {err}"
        );
    }

    #[test]
    fn rejects_invalid_json() {
        let err =
            parse_launch_args(["rollshot-app", "--capture", "not-json"]).expect_err("invalid json");
        assert!(
            err.contains("invalid --capture JSON payload"),
            "err = {err}"
        );
    }

    #[test]
    fn extracts_log_file_before_capture_args() {
        let extracted = extract_logging_args([
            "rollshot-app",
            "--log-file",
            "/tmp/rollshot.jsonl",
            "--capture",
            r#"{"backend":"auto","fps":5,"show_cursor":false}"#,
        ])
        .expect("extract logging args");

        assert_eq!(
            extracted.log_file,
            Some(PathBuf::from("/tmp/rollshot.jsonl"))
        );
        assert_eq!(extracted.remaining[0], "rollshot-app");
        assert_eq!(extracted.remaining[1], "--capture");
    }

    #[test]
    fn rejects_missing_log_file_path() {
        let err = extract_logging_args(["rollshot-app", "--log-file"])
            .expect_err("missing path must fail");
        assert_eq!(err, "--log-file requires a path");
    }

    #[test]
    fn rejects_duplicate_log_file() {
        let err = extract_logging_args([
            "rollshot-app",
            "--log-file",
            "a.jsonl",
            "--log-file",
            "b.jsonl",
        ])
        .expect_err("duplicate option must fail");
        assert_eq!(err, "--log-file may only be specified once");
    }
}
