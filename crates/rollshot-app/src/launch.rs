use rollshot_capture::InteractiveLaunchOptions;
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LaunchMode {
    Capture(InteractiveLaunchOptions),
    #[cfg(feature = "action-guide")]
    ActionGuideProbe,
    #[cfg(feature = "action-guide")]
    ActionGuide,
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

    #[cfg(feature = "action-guide")]
    if flag == "--action-guide" {
        return Ok(LaunchMode::ActionGuide);
    }

    #[cfg(feature = "action-guide")]
    if flag == "--action-guide-probe" {
        return Ok(LaunchMode::ActionGuideProbe);
    }

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

    let value: serde_json::Value = serde_json::from_str(&payload)
        .map_err(|err| format!("invalid --capture JSON payload: {err}"))?;

    if value.get("initial_mode").is_some() {
        return Err(
            "the field `initial_mode` is no longer supported; use `initial_request` \
             with {\"workflow\": \"...\", \"scope\": \"...\"} instead"
                .to_string(),
        );
    }

    let options: InteractiveLaunchOptions = serde_json::from_value(value)
        .map_err(|err| format!("invalid --capture JSON payload: {err}"))?;

    if !options.initial_request.is_supported() {
        return Err(
            "unsupported capture combination: scrolling + fullscreen is not wired; \
             use scrolling + region or screenshot + fullscreen"
                .to_string(),
        );
    }

    Ok(LaunchMode::Capture(options))
}

#[cfg(test)]
mod tests {
    use super::{extract_logging_args, parse_launch_args, LaunchMode};
    use rollshot_capture::CaptureRequest;
    use std::path::PathBuf;

    #[test]
    fn no_args_uses_defaults() {
        let mode = parse_launch_args(["rollshot-app"]).expect("no args should succeed");
        match mode {
            LaunchMode::Capture(options) => {
                assert_eq!(options.backend, "auto");
                assert_eq!(options.fps, 5);
                assert!(!options.show_cursor);
                assert_eq!(options.initial_request, CaptureRequest::scrolling_region());
            }
            #[cfg(feature = "action-guide")]
            LaunchMode::ActionGuideProbe => unreachable!("test expects Capture mode"),
            #[cfg(feature = "action-guide")]
            LaunchMode::ActionGuide => unreachable!("test expects Capture mode"),
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
                assert_eq!(options.initial_request, CaptureRequest::scrolling_region());
            }
            #[cfg(feature = "action-guide")]
            LaunchMode::ActionGuideProbe => unreachable!("test expects Capture mode"),
            #[cfg(feature = "action-guide")]
            LaunchMode::ActionGuide => unreachable!("test expects Capture mode"),
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

    #[test]
    fn fullscreen_capture_request_payload_parses() {
        let mode = parse_launch_args([
            "rollshot-app",
            "--capture",
            r#"{"backend":"auto","fps":5,"show_cursor":false,"initial_request":{"workflow":"screenshot","scope":"fullscreen"}}"#,
        ])
        .unwrap();
        assert!(matches!(
            mode,
            LaunchMode::Capture(options) if options.initial_request == CaptureRequest::screenshot_fullscreen()
        ));
    }

    #[test]
    fn legacy_initial_mode_payload_is_rejected_clearly() {
        let err = parse_launch_args([
            "rollshot-app",
            "--capture",
            r#"{"backend":"auto","fps":5,"show_cursor":false,"initial_mode":"region"}"#,
        ])
        .expect_err("legacy initial_mode should be rejected");
        assert!(err.contains("initial_mode"), "err = {err}");
        assert!(err.contains("initial_request"), "err = {err}");
    }

    #[test]
    fn unsupported_scrolling_fullscreen_payload_is_rejected() {
        let err = parse_launch_args([
            "rollshot-app",
            "--capture",
            r#"{"backend":"auto","fps":5,"show_cursor":false,"initial_request":{"workflow":"scrolling","scope":"fullscreen"}}"#,
        ])
        .expect_err("scrolling+fullscreen should be rejected");
        assert!(err.contains("scrolling"), "err = {err}");
        assert!(err.contains("fullscreen"), "err = {err}");
    }
}
