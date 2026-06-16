//! `rollshot action-guide` — launch the Action Guide recording overlay.
//! Spawns the separate `rollshot-app` GUI binary in record mode.

use crate::args::ActionGuideArgs;
use crate::cli_error::CliError;

pub fn run(_args: &ActionGuideArgs) -> Result<String, CliError> {
    let app = crate::cmd_capture_launcher::resolve_app_binary()?;
    let status = std::process::Command::new(&app)
        .arg("--action-guide")
        .status()
        .map_err(|e| CliError::new(format!("failed to launch {}: {e}", app.display()), 1))?;

    if status.success() {
        Ok("action guide recording completed".to_string())
    } else {
        Err(CliError::new("action guide recording failed", 1))
    }
}
