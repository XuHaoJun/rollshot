//! `rollshot action-guide` — launch the Action Guide recording overlay.
//! Spawns the separate `rollshot-app` GUI binary in record mode.

use crate::args::ActionGuideArgs;
use crate::cli_error::CliError;

/// Flags forwarded to the `rollshot-app` binary for this invocation.
fn extra_args(args: &ActionGuideArgs) -> Vec<String> {
    let mut out = vec!["--action-guide".to_string()];
    if args.fullscreen {
        out.push("--fullscreen".to_string());
    }
    out
}

pub fn run(args: &ActionGuideArgs) -> Result<String, CliError> {
    let app = crate::cmd_capture_launcher::resolve_app_binary()?;
    let status = std::process::Command::new(&app)
        .args(extra_args(args))
        .status()
        .map_err(|e| CliError::new(format!("failed to launch {}: {e}", app.display()), 1))?;

    if status.success() {
        Ok("action guide recording completed".to_string())
    } else {
        Err(CliError::new("action guide recording failed", 1))
    }
}

#[cfg(test)]
mod tests {
    use super::extra_args;
    use crate::args::ActionGuideArgs;

    #[test]
    fn region_mode_passes_only_action_guide() {
        let args = ActionGuideArgs { fullscreen: false };
        assert_eq!(extra_args(&args), vec!["--action-guide".to_string()]);
    }

    #[test]
    fn fullscreen_mode_appends_flag() {
        let args = ActionGuideArgs { fullscreen: true };
        assert_eq!(
            extra_args(&args),
            vec!["--action-guide".to_string(), "--fullscreen".to_string()]
        );
    }
}
